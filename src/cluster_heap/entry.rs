use super::clusters::{ClusterSector, MetaClusterSector};
use crate::error::Error;
use crate::io::IOWrapper;
use crate::region::data::entryset::primary::{DateTime, FileDirectory};
use crate::region::data::entryset::{RawEntry, ENTRY_SIZE};

pub(crate) struct ClusterEntry<IO> {
    pub io: IOWrapper<IO>,
    pub meta: MetaClusterSector,
    pub entry_index: usize,
    pub cluster_sector: ClusterSector<IO>,
    pub sector_size: usize,
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
        let index = self.entry_index;
        let sector_index = self.meta.sector_index();
        let sector = self.io.read(sector_index).await?;
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
        self.io.write(sector_index, offset, &raw_entry).await
    }
}
