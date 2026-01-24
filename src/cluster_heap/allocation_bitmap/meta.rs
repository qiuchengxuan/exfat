use core::mem::transmute;
use core::ops::Deref;

use crate::error::Error;
use crate::io::{self, Block, Wrap};
use crate::region::boot::BootSector;
use crate::sync::Share;
use crate::types::SectorID;

#[derive(Copy, Clone)]
pub struct Meta {
    pub size: u32,
    pub num_clusters: u32,
    pub sector_size_shift: u8,
    pub percent_inuse: u8,
}

impl Meta {
    #[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
    pub(crate) async fn new<B, E, IO, S>(io: S, size: u32) -> Result<Self, Error<E>>
    where
        B: Deref<Target = [Block]>,
        IO: io::IO<Block<'static> = B, Error = E>,
        S: Share<Target = IO>,
    {
        let mut io = io.acquire().await.wrap();
        let blocks = io.read(SectorID::BOOT).await?;
        let boot_sector: &BootSector = unsafe { transmute(&blocks[0]) };
        let sector_size_shift = boot_sector.bytes_per_sector_shift;
        let num_clusters = boot_sector.cluster_count.to_ne();
        let percent_inuse = boot_sector.percent_inuse;
        Ok(Self { size, num_clusters, sector_size_shift, percent_inuse })
    }

    pub fn sector_size(&self) -> u32 {
        1 << self.sector_size_shift
    }

    pub fn num_sectors(&self) -> u32 {
        self.size / self.sector_size()
    }

    pub fn estimate_num_inuse(&self) -> u32 {
        ((self.percent_inuse + 1) as u64 * self.num_clusters as u64 / 100) as u32 - 1
    }
}
