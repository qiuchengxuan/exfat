use core::mem::{size_of, transmute};
use core::ops::Deref;

use memoffset::offset_of;

use crate::error::Error;
use crate::fat::Meta as FAT;
use crate::io::{self, BLOCK_SIZE, Block, Wrap};
use crate::region::boot::BootSector;
use crate::sync::Share;
use crate::types::{Cluster, Sector};

use super::locator::Locator;
use super::meta::Meta;

pub const SEGMENT_SIZE: usize = size_of::<usize>();
pub const SEGMENTS_IN_BLOCK: usize = BLOCK_SIZE / SEGMENT_SIZE;

pub type SegmentChunk = [usize; SEGMENTS_IN_BLOCK];

#[derive(Clone)]
pub struct DumbAllocator<IO> {
    pub(crate) io: IO,
    pub(crate) base: Sector,
    pub(crate) fat: FAT,
    pub(crate) meta: Meta,
    /// Cluster before search base must be allocated
    pub(crate) search_base: Cluster,
    pub(crate) num_inuse: u32,
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<B: Deref<Target = [Block]>, E, IO, S: Share<Target = IO>> DumbAllocator<S>
where
    IO: for<'a> io::IO<Block<'a> = B, Error = E>,
{
    pub(crate) async fn update_usage(&mut self) -> Result<(), Error<E>> {
        let sector_size = self.meta.sector_size();
        let mut num_inuse = 0;
        let mut io = self.io.acquire().await.wrap();
        for sector_offset in 0..self.meta.num_sectors() {
            let sector = io.read(self.base + sector_offset as u64).await?;
            let chunks: &[SegmentChunk] = unsafe { transmute(&*sector) };
            for i in 0..(sector_size as usize / BLOCK_SIZE) {
                let sum = chunks[i].iter().map(|bits| bits.count_ones()).sum::<u32>();
                if !self.search_base.valid() && sum < sector_size {
                    let num_clusters = sector_offset * sector_size + (i * BLOCK_SIZE) as u32;
                    self.search_base = Cluster::FIRST + num_clusters;
                }
                num_inuse += sum;
            }
        }
        self.num_inuse = num_inuse;
        Ok(())
    }

    pub(crate) async fn new(io: S, base: Sector, fat: FAT, meta: Meta) -> Self {
        let search_base = Cluster::FIRST;
        Self { io, base, fat, meta, search_base, num_inuse: meta.estimate_num_inuse() }
    }

    pub(crate) fn locator(&self, cluster: Cluster) -> Locator {
        Locator { meta: self.meta, base: self.base, cluster }
    }

    pub(crate) fn ratio(numerator: u32, dominator: u32) -> u8 {
        core::cmp::min((numerator as u64 * 100 / dominator as u64) as u8, 100)
    }

    pub(crate) async fn ensure_percent_inuse(&mut self) -> Result<(), Error<E>> {
        let offset = offset_of!(BootSector, percent_inuse);
        let percent_inuse = Self::ratio(self.num_inuse, self.meta.num_clusters);
        if percent_inuse as u8 == self.meta.percent_inuse {
            return Ok(());
        }
        self.meta.percent_inuse = percent_inuse as u8;
        let bytes: [u8; 1] = [self.meta.percent_inuse];
        self.io.acquire().await.wrap().write(crate::BOOT_SECTOR, offset, &bytes).await
    }
}
