use derive_more::Display;

use crate::types::{ClusterID, SectorID};

#[derive(Copy, Clone, Debug)]
pub struct Meta {
    pub heap_offset: u32,
    pub sectors_per_cluster_shift: u8,
    pub sector_size_shift: u8,
}

impl Meta {
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

#[derive(Copy, Clone, Debug, Default, Display)]
#[display("{}:{}", cluster, sector)]
pub struct SectorIndex {
    pub cluster: ClusterID,
    pub sector: u32,
}

impl SectorIndex {
    pub fn id(&self, meta: &Meta) -> SectorID {
        let index: u32 = self.cluster.into();
        if index == 0 {
            return SectorID::BOOT; // root directory metadata
        }
        let num_sectors = (index as u64 - 2) * meta.sectors_per_cluster() as u64;
        SectorID::from(meta.heap_offset as u64 + num_sectors + self.sector as u64)
    }

    pub fn new(cluster: ClusterID, sector: u32) -> Self {
        Self { cluster, sector }
    }

    pub fn next(&self, sectors_per_cluster: u32) -> Self {
        if self.sector + 1 > sectors_per_cluster {
            return Self { cluster: self.cluster + 1u32, sector: 0, ..*self };
        }
        Self { sector: self.sector + 1, ..*self }
    }
}
