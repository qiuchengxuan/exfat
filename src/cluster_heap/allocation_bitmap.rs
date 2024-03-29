use core::mem::{size_of, transmute};

use memoffset::offset_of;

use crate::error::{AllocationError, DataError, Error};
use crate::fat;
use crate::io::IOWrapper;
use crate::region::boot::BootSector;
use crate::region::fat::Entry;
use crate::sync::{acquire, Shared};
use crate::types::{ClusterID, SectorID};

const ARRAY_SIZE: usize = 512 / size_of::<usize>();

#[inline]
fn first_zero_bit(bits: u8) -> u8 {
    let bits = !bits;
    ((bits - 1) ^ bits) & bits
}

#[inline]
fn bit_to_offset(bit: u8) -> u8 {
    match bit {
        0b00000001 => return 0,
        0b00000010 => return 1,
        0b00000100 => return 2,
        0b00001000 => return 3,
        0b00010000 => return 4,
        0b00100000 => return 5,
        0b01000000 => return 6,
        0b10000000 => return 7,
        _ => panic!("Not a single bit or is zero"),
    }
}

#[derive(Clone)]
pub struct DumbAllocator<IO> {
    io: Shared<IOWrapper<IO>>,
    base: SectorID,
    fat_info: fat::Info,
    length: u32,
    num_clusters: u32,
    sector_size_shift: u8,
    percent_inuse: u8,
    maybe_available_offset: u32,
    num_inuse_clusters: u32,
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E, IO: crate::io::IO<Error = E>> DumbAllocator<IO> {
    async fn init(&mut self) -> Result<(), Error<E>> {
        let mut sector_id = self.base;
        let mut io = acquire!(self.io);
        let mut sector = io.read(sector_id).await?;
        let mut array: &[[usize; ARRAY_SIZE]] = unsafe { transmute(sector) };
        let sector_size = 1 << self.sector_size_shift;
        let mut num_inuse = 0;
        for i in 0..(self.length as usize / size_of::<usize>()) {
            let index = i % (sector_size / size_of::<usize>());
            if i > 0 && index == 0 {
                sector_id += 1u64;
                sector = io.read(sector_id).await?;
                array = unsafe { transmute(sector) };
            }
            num_inuse += array[index / ARRAY_SIZE][index % ARRAY_SIZE].count_ones();
        }
        self.num_inuse_clusters = num_inuse;
        debug!("Num inuse clusters is {}/{}", num_inuse, self.num_clusters);
        Ok(())
    }

    pub(crate) async fn new(
        io: Shared<IOWrapper<IO>>,
        base: SectorID,
        fat_info: fat::Info,
        length: u32,
    ) -> Result<Self, Error<E>> {
        let mut borrow_io = acquire!(io);
        let blocks = borrow_io.read(0.into()).await?;
        let boot_sector: &BootSector = unsafe { transmute(&blocks[0]) };
        let sector_size_shift = boot_sector.bytes_per_sector_shift;
        let num_clusters = boot_sector.cluster_count.to_ne();
        let percent_inuse = boot_sector.percent_inuse;
        drop(borrow_io);

        let mut bitmap = Self {
            io,
            base,
            fat_info,
            length,
            num_clusters,
            sector_size_shift,
            percent_inuse,
            maybe_available_offset: 0,
            num_inuse_clusters: ((percent_inuse + 1) as u64 * num_clusters as u64 / 100) as u32 - 1,
        };
        if cfg!(feature = "precise-allocation-counter") {
            bitmap.init().await?;
        }
        Ok(bitmap)
    }

    async fn is_available(&mut self, cluster_id: ClusterID) -> Result<Option<u8>, Error<E>> {
        let offset = u32::from(cluster_id) - 2;
        let (byte_offset, bit_offset) = (offset / 8, offset as u8 % 8);
        if byte_offset >= self.length {
            return Ok(None);
        }
        let sector_size = 1 << self.sector_size_shift;
        let sector_id = self.base + offset / 8 / sector_size;
        let mut io = acquire!(self.io);
        let sector = io.read(sector_id).await?;
        let index = (byte_offset % sector_size) as usize;
        let bits = sector[index / 512][index % 512];
        Ok(if bits & (1 << bit_offset) > 0 { Some(bits) } else { None })
    }

    async fn find_available(&mut self) -> Result<(u32, u8), Error<E>> {
        let mut io = acquire!(self.io);
        let sector_size = 1 << self.sector_size_shift;
        let mut sector_id = self.base + self.maybe_available_offset / sector_size;
        let mut sector = io.read(sector_id).await?;
        for i in self.maybe_available_offset..self.length {
            if i != self.maybe_available_offset && i % sector_size == 0 {
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
        let percent_inuse = Self::ratio(self.num_inuse_clusters, self.num_clusters);
        if percent_inuse as u8 == self.percent_inuse {
            return Ok(());
        }
        self.percent_inuse = percent_inuse as u8;
        let bytes: [u8; 1] = [self.percent_inuse];
        acquire!(self.io).write(0.into(), offset, &bytes).await
    }

    pub async fn allocate(&mut self, last: ClusterID, frag: bool) -> Result<ClusterID, Error<E>> {
        if self.maybe_available_offset >= self.length {
            return Err(AllocationError::NoMoreCluster.into());
        }
        let offset = u32::from(last + 1u32) - 2;
        let mut byte_offset = offset / 8;
        let mut bit_offset = offset as u8 % 8;
        let mut bits = 0xFFu8;

        let sector_size = 1 << self.sector_size_shift;
        if last.valid() {
            if let Some(byte) = self.is_available(last + 1u32).await? {
                bits = byte;
            } else if !frag {
                return Err(AllocationError::Fragment.into());
            }
        }
        if bits == 0xFF {
            (byte_offset, bits) = self.find_available().await?;
            bit_offset = bit_to_offset(first_zero_bit(bits));
        };
        let cluster_id = ClusterID::from(byte_offset as u32 * 8 + bit_offset as u32 + 2);
        let sector_id = self.base + byte_offset / sector_size;
        let offset = byte_offset % sector_size;
        bits |= 1 << bit_offset;
        acquire!(self.io).write(sector_id, offset as usize, &[bits; 1]).await?;
        self.num_inuse_clusters += 1;
        self.maybe_available_offset = byte_offset + (bits == 0xFF) as u32;
        if !cfg!(feature = "precise-allocation-counter") {
            if self.maybe_available_offset * 8 > self.num_inuse_clusters {
                self.num_inuse_clusters = self.maybe_available_offset * 8;
            }
        }
        self.ensure_percent_inuse().await?;
        trace!("Allocated cluster {}", cluster_id);
        Ok(cluster_id)
    }

    async fn release_one(&mut self, cluster_id: ClusterID) -> Result<(), Error<E>> {
        trace!("Release cluster id {}", cluster_id);
        let index = u32::from(cluster_id) - 2;
        let byte_offset = index / 8;
        if byte_offset >= self.length {
            warn!("Cluster ID {} out of range", cluster_id);
            return Err(DataError::FATChain.into());
        }
        let mut io = acquire!(self.io);
        let sector_size = 1 << self.sector_size_shift;
        let sector_offset = byte_offset / sector_size;
        let sector_id = self.base + sector_offset;
        let sector = io.read(sector_id).await?;
        let offset = (byte_offset % sector_size) as usize;
        let bit_offset = index % 8;
        let byte = sector[offset / 512][offset % 512] & !(1 << bit_offset);
        io.write(sector_id, offset, &[byte; 1]).await?;
        if byte_offset < self.maybe_available_offset {
            self.maybe_available_offset = byte_offset;
        }
        Ok(())
    }

    pub async fn release(&mut self, cluster_id: ClusterID, chain: bool) -> Result<(), Error<E>> {
        trace!("Release clusters starts with cluster id {}", cluster_id);
        if !chain {
            self.release_one(cluster_id).await?;
            self.ensure_percent_inuse().await?;
            return acquire!(self.io).flush().await;
        }
        let mut cluster_id = cluster_id;
        while cluster_id.valid() {
            self.release_one(cluster_id).await?;
            self.num_inuse_clusters -= 1;
            let sector_id = match self.fat_info.fat_sector_id(cluster_id) {
                Some(id) => id,
                None => return Ok(()),
            };
            let mut io = acquire!(self.io);
            let sector = io.read(sector_id).await?;
            let entry = match self.fat_info.next_cluster_id(sector, cluster_id) {
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
        let mut io = acquire!(self.io);
        return io.flush().await;
    }
}

pub type AllocationBitmap<IO> = DumbAllocator<IO>;
