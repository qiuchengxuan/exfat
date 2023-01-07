use core::mem;
use core::mem::transmute;

#[cfg(feature = "alloc")]
use alloc::rc::Rc;
#[cfg(feature = "std")]
use std::rc::Rc;

use super::clusters::SectorRef;
use super::entry::{ClusterEntry, TouchOption};
use super::file::File;
use crate::error::Error;
use crate::region::data::entry_type::{EntryType, RawEntryType};
use crate::region::data::entryset::primary::{DateTime, FileDirectory};
use crate::region::data::entryset::secondary::{Filename, Secondary, StreamExtension};
use crate::region::data::entryset::RawEntry;
use crate::upcase_table::UpcaseTable;

#[derive(Clone)]
pub struct EntrySet {
    pub name: heapless::String<255>,
    pub file_directory: FileDirectory,
    pub stream_extension: Secondary<StreamExtension>,
    pub(crate) sector_ref: SectorRef,
    pub(crate) entry_index: usize,
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
    pub async fn find<R, H>(&mut self, handler: H) -> Result<Option<R>, Error<E>>
    where
        H: Fn(&EntrySet) -> Option<R>,
    {
        let mut entry_set = EntrySet {
            name: heapless::String::<255>::new(),
            file_directory: Default::default(),
            stream_extension: Default::default(),
            sector_ref: self.entry.sector_ref,
            entry_index: 0,
        };
        let mut sector_ref = self.entry.sector_ref;
        let mut name_length: u8 = 0;
        let mut secondary_remain: u8 = 0;
        let mut sector = self.entry.io.read(sector_ref.id()).await?;
        loop {
            let f = |block| unsafe { transmute::<&[u8; 512], &[RawEntry; 16]>(block) }.iter();
            for (entry_index, entry) in sector.iter().map(f).flatten().enumerate() {
                let entry_type = RawEntryType::new(entry[0]);
                if entry_type.is_end_of_directory() {
                    return Ok(None);
                }
                if !entry_type.in_use() {
                    continue;
                }
                match entry_type.entry_type() {
                    Ok(EntryType::FileDirectory) => {
                        entry_set.file_directory = unsafe { mem::transmute(*entry) };
                        secondary_remain = entry_set.file_directory.secondary_count;
                    }
                    Ok(EntryType::StreamExtension) => {
                        entry_set.stream_extension = unsafe { mem::transmute(*entry) };
                        secondary_remain -= 1;
                        name_length = entry_set.stream_extension.custom_defined.name_length;
                    }
                    Ok(EntryType::Filename) => {
                        let entry: &Filename = unsafe { mem::transmute(entry) };
                        for ch in entry.filename {
                            let ch = unsafe { char::from_u32_unchecked(ch.to_ne() as u32) };
                            entry_set.name.push(ch).ok();
                        }
                        secondary_remain -= 1;
                        if secondary_remain > 0 {
                            continue;
                        }
                        entry_set.entry_index = entry_index;
                        entry_set.name.truncate(name_length as usize);
                        if let Some(r) = handler(&entry_set) {
                            return Ok(Some(r));
                        }
                        entry_set.name.truncate(0);
                    }
                    _ => continue,
                }
            }
            sector_ref = self.entry.next(sector_ref).await?;
            sector = self.entry.io.read(sector_ref.id()).await?;
            entry_set.sector_ref = sector_ref;
        }
    }

    pub async fn touch(&mut self, datetime: DateTime, option: TouchOption) -> Result<(), Error<E>> {
        self.entry.touch(datetime, option).await
    }

    pub async fn open(&mut self, name: &str) -> Result<FileOrDirectory<IO>, Error<E>> {
        let upcase_table = self.upcase_table.clone();
        let future = self.find(|entry_set| -> Option<EntrySet> {
            if !upcase_table.equals(name, entry_set.name.as_str()) {
                return None;
            }
            Some(entry_set.clone())
        });
        let entry_set = future.await?.ok_or(Error::NoSuchFileOrDirectory)?;
        let stream_extension = &entry_set.stream_extension;
        let cluster_id = stream_extension.first_cluster.to_ne();
        let sector_ref = self.entry.sector_ref.new(cluster_id.into(), 0);
        let entry = ClusterEntry {
            io: self.entry.io.clone(),
            #[cfg(any(feature = "async", feature = "std"))]
            allocation_bitmap: self.entry.allocation_bitmap.clone(),
            fat_info: self.entry.fat_info,
            meta: entry_set.sector_ref,
            entry_index: entry_set.entry_index,
            sector_ref,
            sector_size_shift: self.entry.sector_size_shift,
            length: stream_extension.custom_defined.valid_data_length.to_ne(),
            capacity: stream_extension.data_length.to_ne(),
        };
        if entry_set.file_directory.file_attributes().directory() > 0 {
            Ok(FileOrDirectory::Directory(Directory {
                entry,
                upcase_table: self.upcase_table.clone(),
            }))
        } else {
            Ok(FileOrDirectory::File(File {
                entry,
                sector_ref,
                offset: 0,
            }))
        }
    }
}
