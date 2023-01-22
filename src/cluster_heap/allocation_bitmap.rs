use core::mem::{size_of, transmute};

use memoffset::offset_of;

use super::clusters::SectorRef;
use crate::error::Error;
use crate::fat;
use crate::io::IOWrapper;
use crate::region::boot::BootSector;
use crate::region::fat::Entry;
use crate::types::ClusterID;

const ARRAY_SIZE: usize = 512 / size_of::<usize>();

#[inline]
fn first_zero_bit(bits: u8) -> u8 {
    let bits = !bits;
    ((bits - 1) ^ bits) & bits
}

#[inline]
fn bit_offset(bit: u8) -> u8 {
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
    io: IOWrapper<IO>,
    base: SectorRef,
    fat_info: fat::Info,
    maybe_available_offset: u32,
    length: u32,
    sector_size_shift: u8,
    num_clusters: u32,
    percent_inuse: u8,
    num_available_clusters: u32,
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E, IO: crate::io::IO<Error = E>> DumbAllocator<IO> {
    async fn init(&mut self) -> Result<(), Error<E>> {
        let mut sector_id = self.base.id();
        let sector = self.io.read(sector_id).await?;
        if sector[0][0] & 0x3 != 0x3 {
            let byte = sector[0][0] | 0x3;
            self.io.write(self.base.id(), 0, &[byte; 1]).await?;
        }
        let mut sector = self.io.read(sector_id).await?;
        let mut array: &[[usize; ARRAY_SIZE]] = unsafe { transmute(sector) };
        let sector_size = 1 << self.sector_size_shift;
        let mut num_available = self.num_clusters;
        for i in 0..(self.length as usize / size_of::<usize>()) {
            if i > 0 && i % (sector_size / size_of::<usize>()) == 0 {
                sector_id += 1u64;
                sector = self.io.read(sector_id).await?;
                array = unsafe { transmute(sector) };
            }
            let index = i % sector_size;
            num_available -= array[index / ARRAY_SIZE][index % ARRAY_SIZE].count_ones();
        }
        self.num_available_clusters = num_available;
        trace!("Num available clusters is {}/{}", num_available, self.num_clusters);
        Ok(())
    }

    pub(crate) async fn new(
        mut io: IOWrapper<IO>,
        base: SectorRef,
        fat_info: fat::Info,
        length: u32,
    ) -> Result<Self, Error<E>> {
        let blocks = io.read(0.into()).await?;
        let boot_sector: &BootSector = unsafe { transmute(&blocks[0]) };
        let mut bitmap = Self {
            io,
            base,
            fat_info,
            length,
            maybe_available_offset: 0,
            sector_size_shift: boot_sector.bytes_per_sector_shift,
            num_clusters: boot_sector.cluster_count.to_ne(),
            percent_inuse: boot_sector.percent_inuse,
            num_available_clusters: 0,
        };
        bitmap.init().await?;
        Ok(bitmap)
    }

    async fn next_available(&mut self) -> Result<(u32, u8), Error<E>> {
        let sector_size = 1 << self.sector_size_shift;
        let mut sector_id = self.base.id() + self.maybe_available_offset / sector_size;
        let mut sector = self.io.read(sector_id).await?;
        for i in self.maybe_available_offset..self.length {
            if i != self.maybe_available_offset && i % sector_size == 0 {
                sector_id += 1u64;
                sector = self.io.read(sector_id).await?;
            }
            let index = (i % sector_size) as usize;
            let bits = sector[index / 512][index % 512];
            if bits != u8::MAX {
                return Ok((i, bits));
            }
        }
        Err(Error::NoSpace)
    }

    async fn ensure_percent_inuse(&mut self) -> Result<(), Error<E>> {
        let percent_inuse = self.num_available_clusters / 1024 * 100 / self.num_clusters;
        if percent_inuse as u8 == self.percent_inuse {
            return Ok(());
        }
        self.percent_inuse = percent_inuse as u8;
        let offset = offset_of!(BootSector, percent_inuse);
        let bytes: [u8; 1] = [self.percent_inuse];
        self.io.write(0.into(), offset, &bytes).await
    }

    pub async fn allocate(&mut self) -> Result<ClusterID, Error<E>> {
        if self.num_available_clusters == 0 {
            return Err(Error::NoSpace);
        }
        let (byte_offset, mut bits) = self.next_available().await?;
        let bit_offset = bit_offset(first_zero_bit(bits)) as u32;
        let sector_size = 1 << self.sector_size_shift;
        let sector_id = (self.base + (byte_offset / sector_size) as u32).id();
        let offset = byte_offset % sector_size;
        bits |= 1 << bit_offset;
        self.io.write(sector_id, offset as usize, &[bits; 1]).await?;
        self.num_available_clusters -= 1;
        self.maybe_available_offset = byte_offset + (bits == 0xFF) as u32;
        self.ensure_percent_inuse().await?;
        let cluster_id = ClusterID::from(byte_offset as u32 * 8 + bit_offset + 2);
        trace!("Allocated cluster {}", cluster_id);
        Ok(cluster_id)
    }

    async fn release_one(&mut self, cluster_id: ClusterID) -> Result<(), Error<E>> {
        trace!("Release cluster id {}", cluster_id);
        let index = u32::from(cluster_id) - 2;
        let byte_offset = index / 8;
        if byte_offset >= self.length {
            warn!("Cluster ID {} out of range", cluster_id);
            return Err(Error::Generic("Cluster id out of range"));
        }
        let sector_size = 1 << self.sector_size_shift;
        let sector_offset = byte_offset / sector_size;
        let sector_id = (self.base + sector_offset).id();
        let sector = self.io.read(sector_id).await?;
        let offset = (byte_offset % sector_size) as usize;
        let bit_offset = index % 8;
        let byte = sector[offset / 512][offset % 512] & !(1 << bit_offset);
        self.io.write(sector_id, offset, &[byte; 1]).await?;
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
            return self.io.flush().await;
        }
        let mut cluster_id = cluster_id;
        while cluster_id.valid() {
            self.release_one(cluster_id).await?;
            self.num_available_clusters += 1;
            let sector_id = match self.fat_info.fat_sector_id(cluster_id) {
                Some(id) => id,
                None => return Ok(()),
            };
            let sector = self.io.read(sector_id).await?;
            let entry = match self.fat_info.next_cluster_id(sector, cluster_id) {
                Ok(entry) => entry,
                Err(value) => {
                    warn!("Invalid next entry {:X} for cluster id {}", value, cluster_id);
                    return Err(Error::Generic("Not a valid entry"));
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
        return self.io.flush().await;
    }
}

pub type AllocationBitmap<IO> = DumbAllocator<IO>;
