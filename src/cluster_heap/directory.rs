use core::mem;

#[cfg(not(feature = "std"))]
use alloc::rc::Rc;
#[cfg(feature = "std")]
use std::rc::Rc;

use super::clusters::MetaClusterSector;
use super::entry::{ClusterEntry, TouchOption};
use super::file::File;
use super::sectors::Sectors;
use crate::error::Error;
use crate::region::data::entry_type::{EntryType, RawEntryType};
use crate::region::data::entryset::primary::{DateTime, FileDirectory};
use crate::region::data::entryset::secondary::{Filename, Secondary, StreamExtension};
use crate::region::data::entryset::{RawEntry, ENTRY_SIZE};
use crate::upcase_table::UpcaseTable;

#[derive(Clone)]
pub struct EntrySet {
    pub name: heapless::String<255>,
    pub file_directory: FileDirectory,
    pub stream_extension: Secondary<StreamExtension>,
    pub(crate) cluster_sector: MetaClusterSector,
    pub(crate) entry_index: usize,
}

pub(crate) struct Entries<IO> {
    sectors: Sectors<IO>,
    sector_size: usize,
    cursor: usize,
}

impl<E, IO: crate::io::IO<Error = E>> Entries<IO> {
    #[deasync::deasync]
    pub(crate) async fn next(&mut self) -> Result<&RawEntry, Error<E>> {
        let sector = if self.cursor < self.sector_size / ENTRY_SIZE {
            self.cursor += 1;
            self.sectors.current().await?
        } else {
            self.cursor = 1;
            self.sectors.next().await?
        };
        let block = &sector[(self.cursor - 1) / 16];
        let entries: &[RawEntry; 16] = unsafe { core::mem::transmute(block) };
        Ok(&entries[(self.cursor - 1) % 16])
    }

    pub(crate) fn index(&self) -> (MetaClusterSector, usize) {
        (self.sectors.cluster_sector(), self.cursor - 1)
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

impl<E, IO: crate::io::IO<Error = E>> Directory<IO> {
    pub(crate) fn entries(&self) -> Entries<IO> {
        let io = self.entry.io.clone();
        Entries {
            sectors: Sectors::new(io, self.entry.cluster_sector.clone()),
            sector_size: self.entry.sector_size,
            cursor: 0,
        }
    }
}

#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> Directory<IO> {
    pub async fn find<R, F>(&mut self, f: F) -> Result<Option<R>, Error<E>>
    where
        F: Fn(&EntrySet) -> Option<R>,
    {
        let mut entries = self.entries();
        loop {
            let entry = *entries.next().await?;
            let entry_type = RawEntryType::new(entry[0]);
            if entry_type.is_end_of_directory() {
                return Ok(None);
            }
            if !entry_type.in_use() {
                continue;
            }
            let (cluster_sector, entry_index) = entries.index();
            let file_directory: FileDirectory = match entry_type.entry_type() {
                Ok(EntryType::FileDirectory) => unsafe { mem::transmute(entry) },
                _ => continue,
            };
            let mut secondary_remain = file_directory.secondary_count;

            let entry = *entries.next().await?;
            let entry_type = RawEntryType::new(entry[0]);
            let stream_extension: Secondary<StreamExtension> = match entry_type.entry_type() {
                Ok(EntryType::StreamExtension) => unsafe { mem::transmute(entry) },
                _ => return Err(Error::EOF),
            };
            let name_length = stream_extension.custom_defined.name_length;
            secondary_remain -= 1;

            let mut name = heapless::String::<255>::new();
            while secondary_remain > 0 {
                let entry = entries.next().await?;
                let entry_type = RawEntryType::new(entry[0]);
                match entry_type.entry_type() {
                    Ok(EntryType::Filename) => {
                        let entry: &Filename = unsafe { mem::transmute(entry) };
                        for ch in entry.filename {
                            let ch = unsafe { char::from_u32_unchecked(ch.to_ne() as u32) };
                            name.push(ch).ok();
                        }
                        secondary_remain -= 1;
                    }
                    _ => return Err(Error::EOF),
                }
            }
            name.truncate(name_length as usize);
            let entry_set = EntrySet {
                name,
                file_directory,
                stream_extension,
                cluster_sector,
                entry_index,
            };
            if let Some(r) = f(&entry_set) {
                return Ok(Some(r));
            }
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
        let cluster_index = stream_extension.first_cluster.to_ne();
        let cluster_sector = self.entry.cluster_sector.with(cluster_index, 0);
        let entry = ClusterEntry {
            io: self.entry.io.clone(),
            meta: entry_set.cluster_sector,
            entry_index: entry_set.entry_index,
            cluster_sector: cluster_sector.clone(),
            sector_size: self.entry.sector_size,
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
                cluster_sector,
                offset: 0,
            }))
        }
    }
}
