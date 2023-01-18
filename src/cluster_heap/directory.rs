use core::mem;
use core::mem::transmute;

use alloc::rc::Rc;

use super::clusters::SectorRef;
use super::entry::with_context;
use super::entry::{ClusterEntry, TouchOption};
use super::file::File;
use crate::error::Error;
use crate::region::data::entry_type::{EntryType, RawEntryType};
use crate::region::data::entryset::primary::{DateTime, FileDirectory};
use crate::region::data::entryset::secondary::{Filename, Secondary, StreamExtension};
use crate::region::data::entryset::RawEntry;
use crate::upcase_table::UpcaseTable;

#[derive(Clone, Default)]
pub struct EntrySet {
    pub name: heapless::String<255>,
    pub file_directory: FileDirectory,
    pub stream_extension: Secondary<StreamExtension>,
    pub(crate) sector_ref: SectorRef,
    pub(crate) entry_index: usize,
}

impl EntrySet {
    pub fn data_length(&self) -> u64 {
        self.stream_extension.data_length.to_ne()
    }

    pub fn valid_data_length(&self) -> u64 {
        let valid_data_length = self.stream_extension.custom_defined.valid_data_length;
        valid_data_length.to_ne()
    }
}

pub struct Directory<IO> {
    pub(crate) entry: ClusterEntry<IO>,
    pub(crate) upcase_table: Rc<UpcaseTable>,
}

pub enum FileOrDirectory<IO> {
    File(File<IO>),
    Directory(Directory<IO>),
}

#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> Directory<IO> {
    /// walk_fn secondary parameter indicates whether entry is inuse,
    /// return true if stop walking
    pub async fn walk<H>(&mut self, mut walk_fn: H) -> Result<bool, Error<E>>
    where
        H: FnMut(&EntrySet, bool) -> bool,
    {
        let mut name = heapless::String::<255>::new();
        let mut file_directory = FileDirectory::default();
        let mut stream_extension = Secondary::<StreamExtension>::default();
        let mut sector_ref = self.entry.sector_ref;
        let mut name_length: u8 = 0;
        let mut secondary_remain: u8 = 0;
        let mut sector = self.entry.io.read(sector_ref.id()).await?;
        let (mut entryset_sector_ref, mut entry_index) = (self.entry.sector_ref, 0);
        loop {
            let f = |block| unsafe { transmute::<&[u8; 512], &[RawEntry; 16]>(block) }.iter();
            for (index, entry) in sector.iter().map(f).flatten().enumerate() {
                let entry_type = RawEntryType::new(entry[0]);
                if entry_type.is_end_of_directory() {
                    return Ok(true);
                }
                match entry_type.entry_type() {
                    Ok(EntryType::FileDirectory) => {
                        file_directory = unsafe { mem::transmute(*entry) };
                        (entryset_sector_ref, entry_index) = (sector_ref, index);
                        secondary_remain = file_directory.secondary_count;
                    }
                    Ok(EntryType::StreamExtension) => {
                        stream_extension = unsafe { mem::transmute(*entry) };
                        secondary_remain -= 1;
                        name_length = stream_extension.custom_defined.name_length;
                    }
                    Ok(EntryType::Filename) => {
                        let entry: &Filename = unsafe { mem::transmute(entry) };
                        for ch in entry.filename {
                            let ch = unsafe { char::from_u32_unchecked(ch.to_ne() as u32) };
                            name.push(ch).ok();
                        }
                        secondary_remain -= 1;
                    }
                    _ => continue,
                }
                if secondary_remain > 0 {
                    continue;
                }
                name.truncate(name_length as usize);
                let entry_set = EntrySet {
                    name: name.clone(),
                    file_directory,
                    stream_extension: stream_extension.clone(),
                    sector_ref: entryset_sector_ref,
                    entry_index,
                };
                if walk_fn(&entry_set, entry_type.in_use()) {
                    return Ok(false);
                }
                name.truncate(0);
            }
            sector_ref = self.entry.next(sector_ref).await?;
            sector = self.entry.io.read(sector_ref.id()).await?;
        }
    }

    pub async fn find<H>(&mut self, find_fn: H) -> Result<Option<EntrySet>, Error<E>>
    where
        H: Fn(&EntrySet, bool) -> bool,
    {
        let mut found: Option<EntrySet> = None;
        self.walk(|entry_set, in_use| -> bool {
            if find_fn(entry_set, in_use) {
                found = Some(entry_set.clone());
                return true;
            }
            false
        })
        .await?;
        Ok(found)
    }

    pub async fn touch(&mut self, datetime: DateTime, option: TouchOption) -> Result<(), Error<E>> {
        self.entry.touch(datetime, option).await?;
        self.entry.io.flush().await
    }

    pub async fn open(&mut self, name: &str) -> Result<FileOrDirectory<IO>, Error<E>> {
        let upcase_table = self.upcase_table.clone();
        let future = self.find(|entry_set, in_use| -> bool {
            in_use && upcase_table.equals(name, entry_set.name.as_str())
        });
        let entry_set = future.await?.ok_or(Error::NoSuchFileOrDirectory)?;
        let stream_extension = &entry_set.stream_extension;
        let cluster_id = stream_extension.first_cluster.to_ne();
        let sector_ref = self.entry.sector_ref.new(cluster_id.into(), 0);
        if !with_context!(self.entry.context).add_entry(sector_ref.cluster_id) {
            return Err(Error::AlreadyOpen);
        }
        let entry = ClusterEntry {
            io: self.entry.io.clone(),
            context: self.entry.context.clone(),
            fat_info: self.entry.fat_info,
            meta: entry_set.sector_ref,
            entry_index: entry_set.entry_index,
            sector_ref,
            sector_size_shift: self.entry.sector_size_shift,
            last_cluster_id: 0.into(),
            length: stream_extension.custom_defined.valid_data_length.to_ne(),
            capacity: stream_extension.data_length.to_ne(),
            closed: false,
        };
        if entry_set.file_directory.file_attributes().directory() > 0 {
            Ok(FileOrDirectory::Directory(Directory {
                entry,
                upcase_table: self.upcase_table.clone(),
            }))
        } else {
            let size = entry.length;
            Ok(FileOrDirectory::File(File {
                entry,
                sector_ref,
                cursor: 0,
                size,
            }))
        }
    }

    pub async fn close(self) -> Result<(), Error<E>> {
        self.entry.close().await
    }
}
