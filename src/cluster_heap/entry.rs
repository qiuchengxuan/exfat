use super::clusters::{ClusterSector, Clusters};

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
