use core::mem::{size_of, transmute};
use core::ops::{BitXor, Deref, Sub};

use memoffset::offset_of;

use crate::error::{AllocationError, DataError, Error};
use crate::fat::Info as FAT;
use crate::io::{self, BLOCK_SIZE, Block, Wrap};
use crate::region::boot::BootSector;
use crate::region::fat::Entry;
use crate::sync::Shared;
use crate::types::{ClusterID, SectorID};

const BITMAP_SIZE: usize = BLOCK_SIZE / size_of::<usize>();

#[inline]
fn lsb<T: Copy + From<u8> + Sub<T, Output = T> + BitXor<T, Output = T>>(bits: T) -> T {
    (bits - T::from(1)) ^ bits
}

#[inline]
fn bit_to_offset(bit: u8) -> u8 {
    match bit {
        0b00000001 => 0,
        0b00000010 => 1,
        0b00000100 => 2,
        0b00001000 => 3,
        0b00010000 => 4,
        0b00100000 => 5,
        0b01000000 => 6,
        0b10000000 => 7,
        _ => unreachable!("Not a single bit or is zero"),
    }
}

#[derive(Copy, Clone)]
pub(crate) struct Meta {
    size: u32,
    num_clusters: u32,
    sector_size_shift: u8,
    percent_inuse: u8,
}

impl Meta {
    #[cfg_attr(not(feature = "async"), deasync::deasync)]
    pub(crate) async fn new<B, E, IO>(io: Shared<IO>, size: u32) -> Result<Self, Error<E>>
    where
        B: Deref<Target = [Block]>,
        IO: io::IO<Block = B, Error = E>,
    {
        let mut io = io.acquire().await.wrap();
        let blocks = io.read(SectorID::BOOT).await?;
        let boot_sector: &BootSector = unsafe { transmute(&blocks[0]) };
        let sector_size_shift = boot_sector.bytes_per_sector_shift;
        let num_clusters = boot_sector.cluster_count.to_ne();
        let percent_inuse = boot_sector.percent_inuse;
        Ok(Self { size, num_clusters, sector_size_shift, percent_inuse })
    }
}

#[derive(Clone)]
pub struct DumbAllocator<IO> {
    io: Shared<IO>,
    base: SectorID,
    fat: FAT,
    cursor: ClusterID,
    meta: Meta,
    num_inuse: u32,
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<B: Deref<Target = [Block]>, E, IO: io::IO<Block = B, Error = E>> DumbAllocator<IO> {
    #[inline]
    fn sector_size(&self) -> u32 {
        1 << self.meta.sector_size_shift
    }

    #[inline]
    fn num_sectors(&self) -> u32 {
        self.meta.size / self.sector_size()
    }

    pub(crate) async fn update_usage(&mut self) -> Result<(), Error<E>> {
        let sector_size = self.sector_size();
        let mut num_inuse = 0;
        let mut io = self.io.acquire().await.wrap();
        for sector_offset in 0..self.num_sectors() {
            let sector_id = self.base + sector_offset;
            let sector = io.read(sector_id).await?;
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

    pub(crate) async fn new(io: Shared<IO>, base: SectorID, fat: FAT, meta: Meta) -> Self {
        let num_inuse =
            ((meta.percent_inuse + 1) as u64 * meta.num_clusters as u64 / 100) as u32 - 1;
        Self { io, base, fat, meta, cursor: ClusterID::FIRST, num_inuse }
    }

    async fn is_available(&mut self, cluster_id: ClusterID) -> Result<Option<u8>, Error<E>> {
        let offset = u32::from(cluster_id) - 2;
        let (byte_offset, bit_offset) = (offset / 8, offset as u8 % 8);
        if byte_offset >= self.meta.size {
            return Ok(None);
        }
        let sector_size = 1 << self.meta.sector_size_shift;
        let sector_id = self.base + offset / 8 / sector_size;
        let mut io = self.io.acquire().await.wrap();
        let sector = io.read(sector_id).await?;
        let index = (byte_offset % sector_size) as usize;
        let bits = sector[index / 512][index % 512];
        Ok(if bits & (1 << bit_offset) > 0 { Some(bits) } else { None })
    }

    async fn find_available(&mut self) -> Result<(u32, u8), Error<E>> {
        let mut io = self.io.acquire().await.wrap();
        let sector_size = 1 << self.meta.sector_size_shift;
        let mut sector_id = self.base + self.cursor.offset() / sector_size;
        let mut sector = io.read(sector_id).await?;
        for i in self.cursor.offset()..self.meta.size {
            if i != self.cursor.offset() && i % sector_size == 0 {
                sector_id += 1u64;
                sector = io.read(sector_id).await?;
            }
            let index = (i % sector_size) as usize;
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
        let mut cursor = nofrag.unwrap_or(self.cursor);
        let mut bits = 0xFFu8;

        let sector_size = 1 << self.meta.sector_size_shift;
        if let Some(byte) = self.is_available(cursor + 1u32).await? {
            bits = byte;
        } else if nofrag.is_some() {
            return Err(AllocationError::Fragment.into());
        }
        if bits == 0xFF {
            let (byte_offset, bits) = self.find_available().await?;
            cursor = ClusterID::FIRST + byte_offset * 8 + bit_to_offset(lsb(!bits));
        };
        let offset = cursor.offset();
        let sector_id = self.base + offset / sector_size;
        let offset = (offset / 8) % sector_size;
        bits |= 1 << (offset % 8);
        self.io.acquire().await.wrap().write(sector_id, offset as usize, &[bits; 1]).await?;
        self.num_inuse += 1;
        self.cursor = cursor + (bits == 0xFF) as u32;
        self.ensure_percent_inuse().await?;
        trace!("Allocated cluster {}", cursor);
        Ok(cursor)
    }

    async fn release_one(&mut self, cluster_id: ClusterID) -> Result<(), Error<E>> {
        trace!("Release cluster id {}", cluster_id);
        let cluster_offset = cluster_id.offset();
        let byte_offset = cluster_offset / 8;
        if byte_offset >= self.meta.size {
            warn!("Cluster ID {} out of range", cluster_id);
            return Err(DataError::FATChain.into());
        }
        let mut io = self.io.acquire().await.wrap();
        let sector_size = 1 << self.meta.sector_size_shift;
        let sector_offset = byte_offset / sector_size;
        let sector_id = self.base + sector_offset;
        let sector = io.read(sector_id).await?;
        let offset = (byte_offset % sector_size) as usize;
        let bit_offset = cluster_offset % 8;
        let byte = sector[offset / 512][offset % 512] & !(1 << bit_offset);
        io.write(sector_id, offset, &[byte; 1]).await?;
        Ok(())
    }

    pub async fn release(&mut self, cluster_id: ClusterID, chain: bool) -> Result<(), Error<E>> {
        trace!("Release clusters starts with cluster id {}", cluster_id);
        if !chain {
            self.release_one(cluster_id).await?;
            self.ensure_percent_inuse().await?;
            return self.io.acquire().await.wrap().flush().await;
        }
        let mut cluster_id = cluster_id;
        while cluster_id.valid() {
            self.release_one(cluster_id).await?;
            self.num_inuse -= 1;
            let sector_id = match self.fat.fat_sector_id(cluster_id) {
                Some(id) => id,
                None => return Ok(()),
            };
            let mut io = self.io.acquire().await.wrap();
            let sector = io.read(sector_id).await?;
            let entry = match self.fat.next_cluster_id(&sector, cluster_id) {
                Ok(entry) => entry,
                Err(value) => {
                    warn!("Invalid next entry {:X} for cluster id {}", value, cluster_id);
                    return Err(DataError::FATChain.into());
                }
            };
            match entry {
                Entry::Next(id) => cluster_id = id.into(),
                Entry::Last => break,
                Entry::BadCluster => {
                    warn!("Encountered bad cluster for cluster-id {}", cluster_id);
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
