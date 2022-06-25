use super::clusters::{ClusterSector, Clusters};
use crate::error::Error;
use crate::region::data::entryset::primary::{DateTime, FileDirectory};
use crate::region::data::entryset::{RawEntry, ENTRY_SIZE};

#[derive(Copy, Clone)]
pub(crate) struct EntryIndex {
    pub cluster_sector: ClusterSector,
    pub index: usize,
}

impl EntryIndex {
    pub fn new(cluster_sector: ClusterSector, index: usize) -> Self {
        Self {
            cluster_sector,
            index,
        }
    }

    pub fn invalid() -> Self {
        Self {
            cluster_sector: 0.into(),
            index: 0,
        }
    }
}

pub(crate) struct ClusterEntry<IO> {
    pub io: IO,
    pub clusters: Clusters,
    pub meta_entry: EntryIndex,
    pub cluster_index: u32,
    pub length: u64,
    pub capacity: u64,
}

#[derive(Copy, Clone)]
pub struct TouchOption {
    pub access: bool,
    pub modified: bool,
}

impl Default for TouchOption {
    fn default() -> Self {
        Self {
            access: true,
            modified: true,
        }
    }
}

#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> ClusterEntry<IO> {
    pub async fn touch(&mut self, datetime: DateTime, option: TouchOption) -> Result<(), Error<E>> {
        let index = self.meta_entry.index;
        let sector_index = self.clusters.sector_index(self.meta_entry.cluster_sector);
        let sector = self.io.read(sector_index).await.map_err(|e| Error::IO(e))?;
        let entries: &[[RawEntry; 16]] = unsafe { core::mem::transmute(sector) };
        let raw_entry = entries[index / 16][index % 16];
        let mut file_directory: FileDirectory = unsafe { core::mem::transmute(raw_entry) };
        if option.access {
            file_directory.update_last_accessed_timestamp(datetime);
        }
        if option.modified {
            file_directory.update_last_modified_timestamp(datetime);
        }
        let offset = index * ENTRY_SIZE;
        let result = self.io.write(sector_index, offset, &raw_entry).await;
        result.map_err(|e| Error::IO(e))
    }
}
