use crate::error::Error;
use crate::fat::FAT;
use crate::region::fat::Entry;

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct MetaClusterSector {
    pub heap_offset: u32,
    pub sectors_per_cluster: u32,
    pub cluster_index: u32,
    pub sector_index: u32,
}

impl MetaClusterSector {
    pub fn sector_index(&self) -> u64 {
        self.heap_offset as u64
            + (self.cluster_index - 2) as u64 * self.sectors_per_cluster as u64
            + self.sector_index as u64
    }
}

#[derive(Clone)]
pub(crate) struct ClusterSector<IO> {
    meta: MetaClusterSector,
    fat: FAT<IO>,
}

impl<E, IO: crate::io::IO<Error = E>> ClusterSector<IO> {
    pub fn new(meta: MetaClusterSector, fat: FAT<IO>) -> Self {
        Self { meta, fat }
    }

    pub fn sector_index(&self) -> u64 {
        self.meta.sector_index()
    }

    pub fn meta(&self) -> MetaClusterSector {
        self.meta
    }

    pub fn with(&mut self, cluster_index: u32, sector_index: u32) -> Self {
        Self {
            meta: MetaClusterSector {
                cluster_index,
                sector_index,
                ..self.meta
            },
            fat: self.fat.clone(),
        }
    }

    #[deasync::deasync]
    pub async fn next_sector_index(&mut self) -> Result<u64, Error<E>> {
        self.meta.sector_index += 1;
        if self.meta.sector_index >= self.meta.sectors_per_cluster {
            self.meta.cluster_index = match self.fat.next_cluster(self.meta.cluster_index).await? {
                Entry::Next(index) => Ok(index),
                Entry::Last => Err(Error::EOF),
                Entry::BadCluster => Err(Error::BadFAT),
            }?;
            self.meta.sector_index = 0;
        }
        return Ok(self.sector_index());
    }
}
