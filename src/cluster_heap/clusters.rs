#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct ClusterSector {
    pub heap_offset: u32,
    pub sectors_per_cluster: u32,
    pub cluster_index: u32,
    pub sector_index: u32,
}

impl ClusterSector {
    pub fn sector_index(&self) -> u64 {
        self.heap_offset as u64
            + (self.cluster_index - 2) as u64 * self.sectors_per_cluster as u64
            + self.sector_index as u64
    }

    pub fn new_cluster(&self, cluster_index: u32) -> Self {
        Self {
            cluster_index,
            sector_index: 0,
            ..*self
        }
    }

    pub fn next_cluster(&mut self, cluster_index: u32) {
        self.cluster_index = cluster_index;
        self.sector_index = 0;
    }

    pub fn next_sector(&mut self) -> bool {
        if self.sector_index + 1 > self.sectors_per_cluster {
            return false;
        }
        self.sector_index += 1;
        true
    }
}
