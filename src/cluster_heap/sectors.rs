use super::clusters::{ClusterSector, Clusters};
use crate::error::Error;

pub(crate) struct Sectors<IO> {
    io: IO,
    clusters: Clusters,
    pub cluster_sector: ClusterSector,
}

impl<E, IO: crate::io::IO<Error = E>> Sectors<IO> {
    pub fn new(io: IO, cluster_index: u32, clusters: Clusters) -> Self {
        Self {
            io,
            clusters,
            cluster_sector: cluster_index.into(),
        }
    }

    #[deasync::deasync]
    pub async fn current(&mut self) -> Result<&[u8], Error<E>> {
        let sector_index = self.clusters.sector_index(self.cluster_sector);
        self.io.read(sector_index).await.map_err(|e| Error::IO(e))
    }

    #[deasync::deasync]
    pub async fn next(&mut self) -> Result<&[u8], Error<E>> {
        self.cluster_sector = self
            .clusters
            .next(&mut self.io, self.cluster_sector)
            .await?;
        let sector_index = self.clusters.sector_index(self.cluster_sector);
        self.io.read(sector_index).await.map_err(|e| Error::IO(e))
    }
}
