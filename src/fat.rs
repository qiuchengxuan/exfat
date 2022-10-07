use crate::region::fat::Entry;

#[derive(Copy, Clone, Debug)]
pub(crate) struct FAT {
    sector_size_shift: u8,
    offset: u32,
    length: u32,
}

#[deasync::deasync]
impl FAT {
    pub fn new(sector_size_shift: u8, offset: u32, length: u32) -> Self {
        Self {
            sector_size_shift,
            offset,
            length,
        }
    }

    pub fn sector(&self, cluster_index: u32) -> Option<u32> {
        let index = (cluster_index + 2) / ((1 << self.sector_size_shift as u32) / 4);
        if index >= self.length {
            return None;
        }
        Some(self.offset + index)
    }

    pub fn offset(&self, cluster_index: u32) -> usize {
        (cluster_index as usize + 2) * 4 % (1 << self.sector_size_shift)
    }

    pub fn next_cluster(&mut self, sector: &[[u8; 512]], cluster_index: u32) -> Result<Entry, u32> {
        let offset = (cluster_index as usize + 2) % ((1 << self.sector_size_shift) / 4);
        let array: &[u32; 128] = unsafe { core::mem::transmute(&sector[offset / 128]) };
        Entry::try_from(u32::from_le(array[offset % 128]))
    }
}
