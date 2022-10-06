#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct SectorIndex {
    pub heap_offset: u32,
    pub sectors_per_cluster: u32,
    pub cluster: u32,
    pub sector: u32,
}

impl SectorIndex {
    pub fn sector(&self) -> u64 {
        self.heap_offset as u64
            + (self.cluster - 2) as u64 * self.sectors_per_cluster as u64
            + self.sector as u64
    }

    pub fn with_cluster(&self, cluster: u32) -> Self {
        Self {
            cluster,
            sector: 0,
            ..*self
        }
    }

    pub fn set_cluster(&mut self, cluster: u32) {
        self.cluster = cluster;
        self.sector = 0;
    }

    pub fn next(&mut self) -> bool {
        if self.sector + 1 > self.sectors_per_cluster {
            return false;
        }
        self.sector += 1;
        true
    }
}

impl core::ops::Add<u32> for SectorIndex {
    type Output = Self;

    fn add(self, value: u32) -> Self {
        Self {
            cluster: self.cluster + value / self.sectors_per_cluster,
            sector: value % self.sectors_per_cluster,
            ..self
        }
    }
}

impl core::ops::AddAssign<u32> for &mut SectorIndex {
    fn add_assign(&mut self, value: u32) {
        self.cluster += value / self.sectors_per_cluster;
        self.sector = value % self.sectors_per_cluster;
    }
}
