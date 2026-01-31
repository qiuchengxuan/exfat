use crate::io::Block;
use crate::region::fat::Entry;
use crate::types::Cluster;

#[derive(Copy, Clone, Debug)]
pub(crate) struct Meta {
    sector_size_shift: u8,
    offset: u32,
    length: u32,
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl Meta {
    pub fn new(sector_size_shift: u8, offset: u32, length: u32) -> Self {
        Self { sector_size_shift, offset, length }
    }

    fn sector_size(&self) -> u32 {
        1 << self.sector_size_shift
    }

    pub fn fat_sector(&self, cluster: Cluster) -> Option<u64> {
        let index: u32 = cluster.into();
        let sector_index = index / (self.sector_size() / 4);
        if sector_index >= self.length {
            return None;
        }
        Some((self.offset + sector_index) as u64)
    }

    pub fn offset(&self, cluster: Cluster) -> usize {
        let index: u32 = cluster.into();
        (index * 4 % self.sector_size()) as usize
    }

    pub fn next_cluster_id(&self, sector: &[Block], cluster: Cluster) -> Result<Entry, u32> {
        let index = self.offset(cluster) / 4;
        let array: &[u32; 128] = unsafe { core::mem::transmute(&sector[index / 128]) };
        Entry::try_from(u32::from_le(array[index % 128]))
    }
}
