use super::clusters::{ClusterSector, MetaClusterSector};
use crate::error::Error;
use crate::io::IOWrapper;

pub(crate) struct Sectors<IO> {
    io: IOWrapper<IO>,
    cluster_sector: ClusterSector<IO>,
}

impl<E, IO: crate::io::IO<Error = E>> Sectors<IO> {
    pub fn new(io: IOWrapper<IO>, cluster_sector: ClusterSector<IO>) -> Self {
        Self { io, cluster_sector }
    }

    pub fn cluster_sector(&self) -> MetaClusterSector {
        self.cluster_sector.meta()
    }

    #[deasync::deasync]
    pub async fn current(&mut self) -> Result<&[[u8; 512]], Error<E>> {
        self.io.read(self.cluster_sector.sector_index()).await
    }

    #[deasync::deasync]
    pub async fn next(&mut self) -> Result<&[[u8; 512]], Error<E>> {
        let sector_index = self.cluster_sector.next_sector_index().await?;
        self.io.read(sector_index).await
    }
}
