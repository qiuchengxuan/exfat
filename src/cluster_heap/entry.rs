use super::clusters::Clusters;

pub(crate) struct ClusterEntry<IO> {
    pub io: IO,
    pub clusters: Clusters,
    pub cluster_index: u32,
    pub length: u64,
    pub size: u64,
}
