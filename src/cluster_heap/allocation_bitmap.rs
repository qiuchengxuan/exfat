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

const BITMAP_SIZE: usize = BLOCK_SIZE / size_of::<usize>();

#[inline]
fn lsb<T: Copy + From<u8> + Sub<T, Output = T> + BitXor<T, Output = T>>(bits: T) -> T {
    (bits - T::from(1)) ^ bits
}

#[inline]
fn bit_to_offset(bit: u8) -> u8 {
    u8::trailing_zeros(bit) as u8
}

#[derive(Copy, Clone)]
pub(crate) struct Meta {
    size: u32,
    num_clusters: u32,
    sector_size_shift: u8,
    percent_inuse: u8,
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

    #[inline]
    fn sector_size(&self) -> u32 {
        1 << self.sector_size_shift
    }

    #[inline]
    fn num_sectors(&self) -> u32 {
        self.size / self.sector_size()
    }

    #[inline]
    fn num_inuse(&self) -> u32 {
        ((self.percent_inuse + 1) as u64 * self.num_clusters as u64 / 100) as u32 - 1
    }
}

#[derive(Copy, Clone, derive_more::Display)]
#[display("{cluster}")]
struct Position {
    meta: Meta,
    base: SectorID,
    pub cluster: ClusterID,
}

impl Position {
    #[inline]
    fn sector(&self) -> SectorID {
        self.base + self.cluster.offset() / 8 / self.meta.sector_size()
    }

    #[inline]
    fn byte(&self) -> usize {
        ((self.cluster.offset() / 8) % self.meta.sector_size()) as usize
    }

    fn bit(&self) -> usize {
        (self.cluster.offset() % 8) as usize
    }

    #[inline]
    fn out_of_range(&self) -> bool {
        self.cluster.offset() / 8 >= self.meta.size
    }

    fn bits(&self, block: &[Block]) -> u8 {
        let index = self.byte();
        block[index / 512][index % 512]
    }

    fn is_clear(&self, block: &[Block]) -> Option<u8> {
        let bits = self.bits(block);
        if bits & (1 << self.bit()) == 0 { Some(bits) } else { None }
    }

    fn advance(&mut self) -> Self {
        self.cluster += 1u32;
        *self
    }
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
        Self { io, base, fat, meta, cursor: ClusterID::FIRST, num_inuse: meta.num_inuse() }
    }

    fn position(&self, cluster: ClusterID) -> Position {
        Position { meta: self.meta, base: self.base, cluster }
    }

    async fn is_available(&mut self, position: Position) -> Result<Option<u8>, Error<E>> {
        if position.out_of_range() {
            return Ok(None);
        }
        let mut io = self.io.acquire().await.wrap();
        Ok(position.is_clear(&io.read(position.sector()).await?))
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

    pub async fn allocate(&mut self, nofrag: Option<ClusterID>) -> Result<ClusterID, Error<E>> {
        if self.meta.percent_inuse == 100 {
            return Err(AllocationError::NoMoreCluster.into());
        }
        let mut pos = self.position(nofrag.unwrap_or(self.cursor));
        let mut bits = 0xFFu8;

        if let Some(byte) = self.is_available(pos.advance()).await? {
            bits = byte;
        } else if nofrag.is_some() {
            return Err(AllocationError::Fragment.into());
        }
        if bits == 0xFF {
            let (byte_offset, bits) = self.find_available().await?;
            pos.cluster = ClusterID::FIRST + byte_offset * 8 + bit_to_offset(lsb(!bits));
        };
        bits |= 1 << pos.bit();
        self.io.acquire().await.wrap().write(pos.sector(), pos.byte(), &[bits; 1]).await?;
        self.num_inuse += 1;
        self.cursor = pos.cluster + (bits == 0xFF) as u32;
        self.ensure_percent_inuse().await?;
        trace!("Allocated cluster {}", pos.cluster);
        Ok(pos.cluster)
    }

    async fn release_one(&mut self, position: Position) -> Result<(), Error<E>> {
        trace!("Release cluster {}", position);
        if position.out_of_range() {
            warn!("Cluster ID {} out of range", position);
            return Err(DataError::FATChain.into());
        }
        let mut io = self.io.acquire().await.wrap();
        let sector = io.read(position.sector()).await?;
        let byte = position.bits(&sector) & !(1 << position.bit());
        io.write(position.sector(), position.byte(), &[byte; 1]).await?;
        Ok(())
    }

    pub async fn release(&mut self, cluster: ClusterID, chain: bool) -> Result<(), Error<E>> {
        let mut position = self.position(cluster);
        trace!("Release clusters starts with cluster id {}", position);
        if !chain {
            self.release_one(position).await?;
            self.ensure_percent_inuse().await?;
            return self.io.acquire().await.wrap().flush().await;
        }
        while position.cluster.valid() {
            self.release_one(position).await?;
            self.num_inuse -= 1;
            let sector_id = match self.fat.fat_sector_id(position.cluster) {
                Some(id) => id,
                None => return Ok(()),
            };
            let mut io = self.io.acquire().await.wrap();
            let sector = io.read(sector_id).await?;
            let entry = match self.fat.next_cluster_id(&sector, position.cluster) {
                Ok(entry) => entry,
                Err(value) => {
                    warn!("Invalid next entry {:X} for cluster id {}", value, position);
                    return Err(DataError::FATChain.into());
                }
            };
            match entry {
                Entry::Next(id) => position.cluster = id.into(),
                Entry::Last => break,
                Entry::BadCluster => {
                    warn!("Encountered bad cluster for cluster id {}", position);
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
