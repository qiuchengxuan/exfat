use core::ops::Deref;

use crate::cluster_heap::allocation_bitmap::bitmap::{Bitmap, LittleEndian};
use crate::error::{DataError, Error};
use crate::io::{self, Block, Wrap};
use crate::sync::Share;
use crate::types::Cluster;

use super::allocator::DumbAllocator;
use super::locator::Locator;

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<B: Deref<Target = [Block]>, E, IO, S: Share<Target = IO>> DumbAllocator<S>
where
    IO: for<'a> io::IO<Block<'a> = B, Error = E>,
{
    async fn do_release(&mut self, base: Locator, size: u32) -> Result<(), Error<E>> {
        trace!("Release cluster {}", base);
        if base.out_of_range() {
            warn!("Cluster ID {} out of range", base);
            return Err(DataError::FATChain.into());
        }
        let mut io = self.io.acquire().await.wrap();
        let sector = io.read(base.sector()).await?;
        let bitmap = Bitmap::new(&sector, LittleEndian);
        let start = base.in_sector() as usize;
        let end = start + size as usize;
        for (offset, bits) in bitmap.bits().masking(start..end, false) {
            io.write(base.sector(), offset, &bits.to_le_bytes()).await?;
        }
        if base.cluster < self.search_base {
            self.search_base = base.cluster;
        }
        self.num_inuse -= size;
        Ok(())
    }

    pub async fn release(&mut self, cluster: Cluster, chain: bool) -> Result<(), Error<E>> {
        let mut locator = self.locator(cluster);
        trace!("Release clusters starts with cluster {}", locator);
        if !chain {
            self.do_release(locator, 1).await?;
            self.ensure_percent_inuse().await?;
            return self.io.acquire().await.wrap().flush().await;
        }
        while locator.cluster.valid() {
            self.do_release(locator, 1).await?;
            let sector_id = match self.fat.fat_sector(locator.cluster) {
                Some(id) => id,
                None => return Ok(()),
            };
            let mut io = self.io.acquire().await.wrap();
            let sector = io.read(sector_id).await?;
            let entry = match self.fat.next_cluster_id(&sector, locator.cluster) {
                Ok(entry) => entry,
                Err(value) => {
                    warn!("Invalid next entry {:X} for cluster id {}", value, locator);
                    return Err(DataError::FATChain.into());
                }
            };
            match entry {
                crate::region::fat::Entry::Next(id) => locator.cluster = id.into(),
                crate::region::fat::Entry::Last => break,
                crate::region::fat::Entry::BadCluster => {
                    warn!("Encountered bad cluster for cluster id {}", locator);
                    break;
                }
            }
        }
        self.ensure_percent_inuse().await?;
        let mut io = self.io.acquire().await.wrap();
        return io.flush().await;
    }
}
