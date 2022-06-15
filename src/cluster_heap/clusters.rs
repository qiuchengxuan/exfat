use crate::error::Error;
use crate::fat::FAT;
use crate::region::fat::Entry;

#[derive(Copy, Clone, Debug)]
pub(crate) struct ClusterSector(u32, u32);

impl From<u32> for ClusterSector {
    fn from(value: u32) -> Self {
        Self(value, 0)
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Clusters {
    pub fat: FAT,
    pub heap_offset: u32,
    pub sectors_per_cluster: u32,
    pub sector_size: usize,
}

impl Clusters {
    #[deasync::deasync]
    pub(crate) async fn next<E, IO>(
        &self,
        io: &mut IO,
        cluster_sector: ClusterSector,
    ) -> Result<ClusterSector, Error<E>>
    where
        IO: crate::io::IO<Error = E>,
    {
        if cluster_sector.1 + 1 < self.sectors_per_cluster {
            return Ok(ClusterSector(cluster_sector.0, cluster_sector.1 + 1));
        }
        match self.fat.next_cluster(io, cluster_sector.0).await? {
            Entry::Next(cluster_index) => Ok(ClusterSector(cluster_index, 0)),
            Entry::Last => Err(Error::EOF),
            Entry::BadCluster => Err(Error::BadFAT),
        }
    }

    pub(crate) fn sector_index(&self, cluster_sector: ClusterSector) -> u64 {
        self.heap_offset as u64
            + (cluster_sector.0 - 2) as u64 * self.sectors_per_cluster as u64
            + cluster_sector.1 as u64
    }
}
