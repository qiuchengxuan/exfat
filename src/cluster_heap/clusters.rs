use crate::types::{ClusterID, SectorID};

#[derive(Copy, Clone, Debug, Default)]
pub struct SectorRef {
    pub heap_offset: u32,
    pub sectors_per_cluster_shift: u8,
    pub cluster_id: ClusterID,
    pub sector_index: u32,
}

impl SectorRef {
    pub fn id(&self) -> SectorID {
        let index: u32 = self.cluster_id.into();
        let num_sectors = (index as u64 - 2) * (1 << self.sectors_per_cluster_shift);
        SectorID::from(self.heap_offset as u64 + num_sectors + self.sector_index as u64)
    }

    pub fn new(&self, cluster_id: ClusterID, sector_index: u32) -> Self {
        Self { cluster_id, sector_index, ..*self }
    }

    pub fn next(&self) -> Self {
        if self.sector_index + 1 > (1 << self.sectors_per_cluster_shift) {
            return Self { cluster_id: self.cluster_id + 1u32, sector_index: 0, ..*self };
        }
        Self { sector_index: self.sector_index + 1, ..*self }
    }

    pub fn is_last_sector_in_cluster(&self) -> bool {
        self.sector_index == (1 << self.sectors_per_cluster_shift - 1)
    }
}

impl<I: Into<u32>> core::ops::Add<I> for SectorRef {
    type Output = Self;

    fn add(self, rhs: I) -> Self {
        let rhs = rhs.into();
        Self {
            cluster_id: self.cluster_id + rhs / (1 << self.sectors_per_cluster_shift),
            sector_index: rhs % (1 << self.sectors_per_cluster_shift),
            ..self
        }
    }
}

impl<I: Into<u32>> core::ops::AddAssign<I> for SectorRef {
    fn add_assign(&mut self, rhs: I) {
        let rhs = rhs.into();
        self.cluster_id += rhs / (1 << self.sectors_per_cluster_shift);
        self.sector_index = rhs % (1 << self.sectors_per_cluster_shift);
    }
}
