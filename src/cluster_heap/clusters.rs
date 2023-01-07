use crate::types::{ClusterID, SectorID};

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct SectorRef {
    pub heap_offset: u32,
    pub sectors_per_cluster_shift: u8,
    pub cluster_id: ClusterID,
    pub offset: u32,
}

impl SectorRef {
    pub fn id(&self) -> SectorID {
        let index: u32 = self.cluster_id.into();
        let num_sectors = (index as u64 - 2) * (1 << self.sectors_per_cluster_shift);
        SectorID::from(self.heap_offset as u64 + num_sectors + self.offset as u64)
    }

    pub fn new(&self, cluster_id: ClusterID, offset: u32) -> Self {
        Self {
            cluster_id,
            offset,
            ..*self
        }
    }

    pub fn next(&self) -> Option<Self> {
        if self.offset + 1 > (1 << self.sectors_per_cluster_shift) {
            return None;
        }
        Some(Self {
            offset: self.offset + 1,
            ..*self
        })
    }
}

impl<I: Into<u32>> core::ops::Add<I> for SectorRef {
    type Output = Self;

    fn add(self, rhs: I) -> Self {
        let rhs = rhs.into();
        Self {
            cluster_id: self.cluster_id + rhs / (1 << self.sectors_per_cluster_shift),
            offset: rhs % (1 << self.sectors_per_cluster_shift),
            ..self
        }
    }
}

impl<I: Into<u32>> core::ops::AddAssign<I> for &mut SectorRef {
    fn add_assign(&mut self, rhs: I) {
        let rhs = rhs.into();
        self.cluster_id += rhs / (1 << self.sectors_per_cluster_shift);
        self.offset = rhs % (1 << self.sectors_per_cluster_shift);
    }
}
