use super::clusters::{ClusterSector, Clusters};
use crate::error::Error;
use crate::region::data::entryset::primary::{DateTime, FileDirectory};
use crate::region::data::entryset::ENTRY_SIZE;

#[derive(Copy, Clone)]
pub(crate) struct Offset {
    pub cluster_sector: ClusterSector,
    pub offset: usize,
}

impl Offset {
    pub fn new(cluster_sector: ClusterSector, offset: usize) -> Self {
        Self {
            cluster_sector,
            offset,
        }
    }

    pub fn invalid() -> Self {
        Self {
            cluster_sector: 0.into(),
            offset: 0,
        }
    }
}

pub(crate) struct ClusterEntry<IO> {
    pub io: IO,
    pub clusters: Clusters,
    pub meta_offset: Offset,
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
        let offset = self.meta_offset.offset;
        let sector_index = self.clusters.sector_index(self.meta_offset.cluster_sector);
        let sector = self.io.read(sector_index).await.map_err(|e| Error::IO(e))?;
        let slice = &sector[offset..offset + ENTRY_SIZE];
        let mut array: [u8; ENTRY_SIZE] = slice.try_into().map_err(|_| Error::EOF)?;
        let file_directory: &mut FileDirectory = unsafe { core::mem::transmute(&mut array) };
        if option.access {
            file_directory.update_last_accessed_timestamp(datetime);
        }
        if option.modified {
            file_directory.update_last_modified_timestamp(datetime);
        }
        let result = self.io.write(sector_index, offset, &array).await;
        result.map_err(|e| Error::IO(e))
    }
}
