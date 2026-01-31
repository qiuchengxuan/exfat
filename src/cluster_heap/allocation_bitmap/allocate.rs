use core::cmp::min;
use core::ops::Deref;
use core::ops::Range;

use crate::cluster_heap::allocation_bitmap::bitmap::{BitmapIterator, LittleEndian};
use crate::error::AllocationError;
use crate::io::{self, Block, Wrap};
use crate::sync::Share;
use crate::types::{Cluster, Result};

use super::allocator::DumbAllocator;
use super::bitmap::Bitmap;
use super::locator::Locator;

pub type Clusters = Range<Cluster>;

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<B: Deref<Target = [Block]>, E, IO, S: Share<Target = IO>> DumbAllocator<S>
where
    IO: for<'a> io::IO<Block<'a> = B, Error = E>,
{
    async fn nofrag_allocate(&mut self, base: Locator, size: u32) -> Result<bool, E> {
        if base.cluster.index() + size > self.meta.num_clusters {
            return Ok(false);
        }
        let clusters_in_sector = self.meta.sector_size() as usize * 8;
        let mut io = self.io.acquire().await.wrap();
        let mut remain = size as usize;
        let mut locator = base;
        while remain > 0 {
            let block = io.read(locator.sector()).await?;
            let bitmap = Bitmap::new(&block, LittleEndian);
            let start = locator.in_sector() as usize;
            if bitmap.bits().iter().skip(start).take(remain as usize).any(|v| v) {
                return Ok(false);
            }
            let n = min(remain, clusters_in_sector - start);
            locator.cluster += n as u32;
            remain -= n;
        }

        let mut remain = size as usize;
        let mut last = base;
        while remain > 0 {
            let block = io.read(last.sector()).await?;
            let bitmap = Bitmap::new(&block, LittleEndian);
            let start = last.in_sector() as usize;
            let n = min(start + remain, clusters_in_sector) - start;
            for (offset, bits) in bitmap.bits().masking(start..start + n, true) {
                io.write(last.sector(), offset, &bits.to_le_bytes()).await?;
            }
            last.cluster += n as u32;
            remain -= n;
        }
        Ok(true)
    }

    /// Best effort allocate in case of frag
    pub async fn allocate(&mut self, last: Cluster, size: u32, nf: bool) -> Result<Clusters, E> {
        if self.num_inuse + size > self.meta.num_clusters {
            return Err(AllocationError::NoMoreCluster.into());
        }
        if last.valid() {
            let base = self.locator(last + 1u32);
            if self.nofrag_allocate(base, size).await? {
                if self.search_base == last {
                    self.search_base += size;
                }
                self.num_inuse += size;
                self.ensure_percent_inuse().await?;
                return Ok(base.cluster..base.cluster + size);
            }
            if nf {
                return Err(AllocationError::Fragment.into());
            }
        }
        let mut io = self.io.acquire().await.wrap();
        let mut first = self.locator(self.search_base);
        while !first.out_of_range() {
            let block = io.read(first.sector()).await?;
            let skip = first.in_sector();
            let bitmap = Bitmap::new(&block, LittleEndian);
            if let Some(pos) = bitmap.bits().iter().skip(skip as usize).position(|v| !v) {
                first += pos as u32;
                break;
            }
            first += self.meta.sector_size() * 8 - skip;
        }
        // possibly but unlikely
        if first.out_of_range() {
            self.search_base = first.cluster;
            self.num_inuse = self.meta.num_clusters;
            return Err(AllocationError::NoMoreCluster.into());
        }
        let clusters_in_sector = self.meta.sector_size() as usize * 8;
        let mut last = first;
        let first = first.cluster;
        let mut remain = size as usize;
        while remain > 0 && !last.out_of_range() {
            let block = io.read(last.sector()).await?;
            let start = last.in_sector() as usize;
            let bitmap = Bitmap::new(&block, LittleEndian);
            let maybe_pos = bitmap.bits().iter().skip(start).take(remain).position(|v| v);
            let end = min(start + maybe_pos.unwrap_or(remain), clusters_in_sector);
            for (offset, bits) in bitmap.bits().masking(start..end, true) {
                io.write(last.sector(), offset, &bits.to_le_bytes()).await?;
            }
            remain -= end - start;
            last.cluster += (end - start) as u32;
            if maybe_pos.is_some() {
                break;
            }
        }
        if last.cluster > first {
            let n = u32::from(last.cluster - first);
            self.search_base = last.cluster;
            self.num_inuse += n;
            drop(io);
            self.ensure_percent_inuse().await?;
            return Ok(first..last.cluster);
        }
        Err(AllocationError::NoMoreCluster.into())
    }
}
