use core::ops::AddAssign;

use crate::types::{Cluster, Sector};

use super::meta::Meta;

#[derive(Copy, Clone, derive_more::Display)]
#[display("{cluster}")]
pub struct Locator {
    pub meta: Meta,
    pub base: Sector,
    pub cluster: Cluster,
}

impl Locator {
    pub fn sector(&self) -> u64 {
        self.base + (self.cluster.index() / u8::BITS / self.meta.sector_size()) as u64
    }

    pub fn in_sector(&self) -> u32 {
        self.cluster.index() % (self.meta.sector_size() * u8::BITS)
    }

    pub fn out_of_range(&self) -> bool {
        self.cluster.index() >= self.meta.size * u8::BITS
    }
}

impl AddAssign<u32> for Locator {
    fn add_assign(&mut self, rhs: u32) {
        self.cluster += rhs
    }
}
