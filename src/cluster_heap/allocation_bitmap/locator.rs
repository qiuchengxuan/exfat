use super::meta::Meta;
use crate::io::Block;
use crate::types::{ClusterID, Sector};

pub struct InSector(u32);

impl InSector {
    pub fn byte(&self) -> u32 {
        self.0 / 8
    }
}

#[derive(Copy, Clone, derive_more::Display)]
#[display("{cluster}")]
pub struct Locator {
    pub meta: Meta,
    pub base: Sector,
    pub cluster: ClusterID,
}

impl Locator {
    pub fn sector(&self) -> u64 {
        self.base + (self.cluster.offset() / self.meta.sector_size()) as u64
    }

    pub fn in_sector(&self) -> InSector {
        InSector(self.cluster.offset() % self.meta.sector_size())
    }

    pub fn out_of_range(&self) -> bool {
        self.cluster.offset() >= self.meta.size
    }

    pub fn bit(&self) -> usize {
        self.cluster.bit() as usize
    }

    pub fn bits(&self, blocks: &[Block]) -> u8 {
        let index = self.in_sector().byte() as usize;
        crate::io::flatten(blocks)[index]
    }

    pub fn is_clear(&self, block: &[Block]) -> Option<u8> {
        let bits = self.bits(block);
        if bits & (1 << self.bit()) == 0 { Some(bits) } else { None }
    }

    pub fn advance(&mut self) -> Self {
        self.cluster += 1u32;
        *self
    }
}
