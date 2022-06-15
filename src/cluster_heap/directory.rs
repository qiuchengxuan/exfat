use core::mem;

#[cfg(not(feature = "std"))]
use alloc::rc::Rc;
#[cfg(feature = "std")]
use std::rc::Rc;

use super::clusters::ClusterSector;
use super::entry::ClusterEntry;
use super::file::File;
use super::sectors::Sectors;
use crate::error::Error;
use crate::region::data::entry_type::{EntryType, RawEntryType};
use crate::region::data::entryset::primary::FileDirectory;
use crate::region::data::entryset::secondary::{Filename, Secondary, StreamExtension};
use crate::upcase_table::UpcaseTable;

#[derive(Clone, Debug)]
pub struct EntrySet {
    pub name: heapless::String<255>,
    pub file_directory: FileDirectory,
    pub stream_extension: Secondary<StreamExtension>,
    pub(crate) cluster_sector: ClusterSector,
    pub(crate) offset: usize,
}

pub(crate) struct Entries<IO> {
    sectors: Sectors<IO>,
    sector_size: usize,
    cursor: usize,
}

impl<E, IO: crate::io::IO<Error = E>> Entries<IO> {
    #[deasync::deasync]
    pub(crate) async fn next(&mut self) -> Result<&[u8; 32], Error<E>> {
        let sector = if self.cursor < self.sector_size {
            self.cursor += 32;
            self.sectors.current().await?
        } else {
            self.cursor = 32;
            self.sectors.next().await?
        };
        let entry = &sector[self.cursor - 32..self.cursor];
        entry.try_into().map_err(|_| Error::EOF)
    }

    pub(crate) fn offset(&self) -> (ClusterSector, usize) {
        (self.sectors.cluster_sector, self.cursor)
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
            sectors: Sectors::new(io, self.entry.cluster_index, self.entry.clusters),
            sector_size: self.entry.clusters.sector_size,
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
            let (cluster_sector, offset) = entries.offset();
            let chunk = *entries.next().await?;
            let entry_type = RawEntryType::new(chunk[0]);
            if entry_type.is_end_of_directory() {
                return Ok(None);
            }
            if !entry_type.in_use() {
                continue;
            }
            let file_directory: FileDirectory = match entry_type.entry_type() {
                Ok(EntryType::FileDirectory) => unsafe { mem::transmute(chunk) },
                _ => continue,
            };
            let mut secondary_remain = file_directory.secondary_count;

            let chunk = *entries.next().await?;
            let entry_type = RawEntryType::new(chunk[0]);
            let stream_extension: Secondary<StreamExtension> = match entry_type.entry_type() {
                Ok(EntryType::StreamExtension) => unsafe { mem::transmute(chunk) },
                _ => return Err(Error::EOF),
            };
            let name_length = stream_extension.custom_defined.name_length;
            secondary_remain -= 1;

            let mut name = heapless::String::<255>::new();
            while secondary_remain > 0 {
                let chunk = entries.next().await?;
                let entry_type = RawEntryType::new(chunk[0]);
                match entry_type.entry_type() {
                    Ok(EntryType::Filename) => {
                        let entry: &Filename = unsafe { mem::transmute(chunk) };
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
                offset,
            };
            if let Some(r) = f(&entry_set) {
                return Ok(Some(r));
            }
        }
    }

    pub async fn open(&mut self, name: &str) -> Result<FileOrDirectory<IO>, Error<E>> {
        let upcase_table = self.upcase_table.clone();
        let option = self.find(|entry_set| -> Option<EntrySet> {
            if !upcase_table.equals(name, entry_set.name.as_str()) {
                return None;
            }
            Some(entry_set.clone())
        })?;
        let entry_set = option.ok_or(Error::NoSuchFileOrDirectory)?;
        let stream_extension = &entry_set.stream_extension;
        let entry = ClusterEntry {
            io: self.entry.io.clone(),
            clusters: self.entry.clusters,
            cluster_index: stream_extension.first_cluster.to_ne(),
            length: stream_extension.custom_defined.valid_data_length.to_ne(),
            size: stream_extension.data_length.to_ne(),
        };
        if entry_set.file_directory.file_attributes().directory() > 0 {
            Ok(FileOrDirectory::Directory(Directory {
                entry,
                upcase_table: self.upcase_table.clone(),
            }))
        } else {
            Ok(FileOrDirectory::File(File { entry, offset: 0 }))
        }
    }
}
