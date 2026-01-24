pub(crate) mod clusters;
mod locator;
pub(crate) mod meta;

use core::mem::{size_of, transmute};
use core::ops::{BitXor, Deref, Sub};

use memoffset::offset_of;

use crate::error::{AllocationError, DataError, Error};
use crate::fat::Meta as FAT;
use crate::io::{self, BLOCK_SIZE, Block, Wrap};
use crate::region::boot::BootSector;
use crate::region::fat::Entry;
use crate::sync::Share;
use crate::types::{ClusterID, SectorID};

pub use clusters::Clusters;
use locator::Locator;
pub use meta::Meta;

const BITMAP_SIZE: usize = BLOCK_SIZE / size_of::<usize>();

fn lsb(bits: u8) -> u8 {
    bits ^ ((bits - 1) & bits)
}

fn bit_to_offset(bit: u8) -> u8 {
    u8::trailing_zeros(bit) as u8
}

#[derive(Clone)]
pub struct DumbAllocator<IO> {
    io: IO,
    base: SectorID,
    fat: FAT,
    cursor: ClusterID,
    meta: Meta,
    num_inuse: u32,
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<B: Deref<Target = [Block]>, E, IO, S: Share<Target = IO>> DumbAllocator<S>
where
    IO: io::IO<Block<'static> = B, Error = E>,
{
    pub(crate) async fn update_usage(&mut self) -> Result<(), Error<E>> {
        let sector_size = self.meta.sector_size();
        let mut num_inuse = 0;
        let mut io = self.io.acquire().await.wrap();
        for sector_offset in 0..self.meta.num_sectors() {
            let sector = io.read(self.base + sector_offset).await?;
            let blocks: &[[usize; BITMAP_SIZE]] = unsafe { transmute(&*sector) };
            for i in 0..(sector_size as usize / BLOCK_SIZE) {
                let sum = blocks[i].iter().map(|bits| bits.count_ones()).sum::<u32>();
                if !self.cursor.valid() && sum < sector_size {
                    let num_clusters = sector_offset * sector_size + (i * BLOCK_SIZE) as u32;
                    self.cursor = ClusterID::FIRST + num_clusters;
                }
                num_inuse += sum;
            }
        }
        self.num_inuse = num_inuse;
        Ok(())
    }

    pub(crate) async fn new(io: S, base: SectorID, fat: FAT, meta: Meta) -> Self {
        Self { io, base, fat, meta, cursor: ClusterID::FIRST, num_inuse: meta.estimate_num_inuse() }
    }

    fn locator(&self, cluster: ClusterID) -> Locator {
        Locator { meta: self.meta, base: self.base, cluster }
    }

    async fn is_available(&mut self, locator: Locator) -> Result<Option<u8>, Error<E>> {
        if locator.out_of_range() {
            return Ok(None);
        }
        let mut io = self.io.acquire().await.wrap();
        Ok(locator.is_clear(&io.read(locator.sector()).await?))
    }

    async fn find_available(&mut self) -> Result<(u32, u8), Error<E>> {
        let mut io = self.io.acquire().await.wrap();
        let mut sector_id = self.base + self.cursor.offset() / self.meta.sector_size();
        let mut sector = io.read(sector_id).await?;
        for i in self.cursor.offset()..self.meta.size {
            if i != self.cursor.offset() && i % self.meta.sector_size() == 0 {
                sector_id += 1u64;
                sector = io.read(sector_id).await?;
            }
            let index = (i % self.meta.sector_size()) as usize;
            let bits = sector[index / 512][index % 512];
            if bits != u8::MAX {
                return Ok((i, bits));
            }
        }
        Err(AllocationError::NoMoreCluster.into())
    }

    fn ratio(numerator: u32, dominator: u32) -> u8 {
        core::cmp::min((numerator as u64 * 100 / dominator as u64) as u8, 100)
    }

    async fn ensure_percent_inuse(&mut self) -> Result<(), Error<E>> {
        let offset = offset_of!(BootSector, percent_inuse);
        let percent_inuse = Self::ratio(self.num_inuse, self.meta.num_clusters);
        if percent_inuse as u8 == self.meta.percent_inuse {
            return Ok(());
        }
        self.meta.percent_inuse = percent_inuse as u8;
        let bytes: [u8; 1] = [self.meta.percent_inuse];
        self.io.acquire().await.wrap().write(SectorID::BOOT, offset, &bytes).await
    }

    pub async fn allocate(
        &mut self, nofrag: Option<ClusterID>, size: u32,
    ) -> Result<Clusters, Error<E>> {
        if self.num_inuse + size > self.meta.num_clusters {
            return Err(AllocationError::NoMoreCluster.into());
        }
        let mut loc = self.locator(nofrag.unwrap_or(self.cursor));
        let mut bits = 0xFFu8;

        if let Some(byte) = self.is_available(loc.advance()).await? {
            bits = byte;
        } else if nofrag.is_some() {
            return Err(AllocationError::Fragment.into());
        }
        if bits == 0xFF {
            let (byte_offset, bits) = self.find_available().await?;
            loc.cluster = ClusterID::FIRST + byte_offset * 8 + bit_to_offset(lsb(!bits));
        };
        bits |= 1 << loc.bit();
        let offset = loc.in_sector().byte() as usize;
        self.io.acquire().await.wrap().write(loc.sector(), offset, &[bits; 1]).await?;
        self.num_inuse += 1;
        self.cursor = loc.cluster + (bits == 0xFF) as u32;
        self.ensure_percent_inuse().await?;
        trace!("Allocated cluster {}", loc.cluster);
        Ok(Clusters { base: loc.cluster, size: 1, bits: 0 })
    }

    async fn release_one(&mut self, locator: Locator) -> Result<(), Error<E>> {
        trace!("Release cluster {}", locator);
        if locator.out_of_range() {
            warn!("Cluster ID {} out of range", locator);
            return Err(DataError::FATChain.into());
        }
        let mut io = self.io.acquire().await.wrap();
        let sector = io.read(locator.sector()).await?;
        let byte = locator.bits(&sector) & !(1 << locator.bit());
        io.write(locator.sector(), locator.in_sector().byte() as usize, &[byte; 1]).await
    }

    pub async fn release(&mut self, cluster: ClusterID, chain: bool) -> Result<(), Error<E>> {
        let mut locator = self.locator(cluster);
        trace!("Release clusters starts with cluster id {}", locator);
        if !chain {
            self.release_one(locator).await?;
            self.ensure_percent_inuse().await?;
            return self.io.acquire().await.wrap().flush().await;
        }
        while locator.cluster.valid() {
            self.release_one(locator).await?;
            self.num_inuse -= 1;
            let sector_id = match self.fat.fat_sector_id(locator.cluster) {
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
                Entry::Next(id) => locator.cluster = id.into(),
                Entry::Last => break,
                Entry::BadCluster => {
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

pub type AllocationBitmap<IO> = DumbAllocator<IO>;
