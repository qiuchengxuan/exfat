use crate::region::fat::Entry;
use crate::types::{ClusterID, SectorID};

#[derive(Copy, Clone, Debug)]
pub(crate) struct Info {
    sector_size_shift: u8,
    offset: u32,
    length: u32,
}

#[deasync::deasync]
impl Info {
    pub fn new(sector_size_shift: u8, offset: u32, length: u32) -> Self {
        Self {
            sector_size_shift,
            offset,
            length,
        }
    }

    pub fn fat_sector_id(&self, cluster_id: ClusterID) -> Option<SectorID> {
        let index: u32 = cluster_id.into();
        let sector_index = (index + 2) / ((1 << self.sector_size_shift as u32) / 4);
        if sector_index >= self.length {
            return None;
        }
        Some(SectorID::from((self.offset + sector_index) as u64))
    }

    pub fn offset(&self, cluster_id: ClusterID) -> usize {
        let index: u32 = cluster_id.into();
        (index as usize + 2) * 4 % (1 << self.sector_size_shift)
    }

    pub fn next_cluster_id(
        &mut self,
        sector: &[[u8; 512]],
        cluster_id: ClusterID,
    ) -> Result<Entry, u32> {
        let index: u32 = cluster_id.into();
        let offset = (index as usize + 2) % ((1 << self.sector_size_shift) / 4);
        let array: &[u32; 128] = unsafe { core::mem::transmute(&sector[offset / 128]) };
        Entry::try_from(u32::from_le(array[offset % 128]))
    }
}
