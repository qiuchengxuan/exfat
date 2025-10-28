use core::fmt::Display;

use crate::types::{ClusterID, SectorID};

#[derive(Copy, Clone, Debug)]
pub struct Info {
    pub heap_offset: u32,
    pub sectors_per_cluster_shift: u8,
    pub sector_size_shift: u8,
}

impl Info {
    pub fn sector_size(&self) -> u16 {
        1 << self.sector_size_shift
    }

    pub fn sectors_per_cluster(&self) -> u32 {
        1 << self.sectors_per_cluster_shift
    }

    pub fn cluster_size_shift(&self) -> u8 {
        self.sector_size_shift + self.sectors_per_cluster_shift
    }

    pub fn cluster_size(&self) -> u32 {
        1 << self.cluster_size_shift()
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct SectorIndex {
    pub cluster_id: ClusterID,
    pub sector_index: u32,
}

impl Display for SectorIndex {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}", self.cluster_id, self.sector_index)
    }
}

impl SectorIndex {
    pub fn id(&self, fs_info: &Info) -> SectorID {
        let index: u32 = self.cluster_id.into();
        let num_sectors = (index as u64 - 2) * fs_info.sectors_per_cluster() as u64;
        SectorID::from(fs_info.heap_offset as u64 + num_sectors + self.sector_index as u64)
    }

    pub fn new(cluster_id: ClusterID, sector_index: u32) -> Self {
        Self { cluster_id, sector_index }
    }

    pub fn next(&self, sectors_per_cluster_shift: u8) -> Self {
        if self.sector_index + 1 > (1 << sectors_per_cluster_shift) {
            return Self { cluster_id: self.cluster_id + 1u32, sector_index: 0, ..*self };
        }
        Self { sector_index: self.sector_index + 1, ..*self }
    }
}
