use core::mem::{size_of, transmute};

use memoffset::offset_of;

use super::clusters::SectorRef;
use crate::error::Error;
use crate::fat;
use crate::io::IOWrapper;
use crate::region::boot::BootSector;
use crate::region::fat::Entry;
use crate::types::ClusterID;

#[derive(Clone)]
pub struct DumbAllocator<IO> {
    io: IOWrapper<IO>,
    base: SectorRef,
    fat_info: fat::Info,
    length: u32,
    sector_size_shift: u8,
    num_clusters: u32,
    percent_inuse: u8,
    num_available_clusters: u32,
}

#[inline]
fn first_zero_bit(bits: usize) -> usize {
    (bits.wrapping_add(1) ^ bits).next_power_of_two()
}

#[inline]
fn bit_offset(bit: usize) -> usize {
    let bytes: [u8; size_of::<usize>()] = unsafe { transmute(bit) };
    for i in 0..size_of::<usize>() {
        match bytes[i] {
            0b00000001 => return i * 8,
            0b00000010 => return i * 8 + 1,
            0b00000100 => return i * 8 + 2,
            0b00001000 => return i * 8 + 3,
            0b00010000 => return i * 8 + 4,
            0b00100000 => return i * 8 + 5,
            0b01000000 => return i * 8 + 6,
            0b10000000 => return i * 8 + 7,
            _ => continue,
        }
    }
    panic!("Not a single bit or is zero")
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E, IO: crate::io::IO<Error = E>> DumbAllocator<IO> {
    async fn init(&mut self) -> Result<(), Error<E>> {
        let sector = self.io.read(self.base.id()).await?;
        if sector[0][0] & 0x3 != 0x3 {
            let byte = sector[0][0] | 0x3;
            self.io.write(self.base.id(), 0, &[byte; 1]).await?;
        }
        let mut num_available = 0;
        for i in 0..self.length / (1 << self.sector_size_shift) {
            let sector = self.io.read((self.base + i).id()).await?;
            for block in sector.iter() {
                let array: &[usize; 512 / size_of::<usize>()] = unsafe { transmute(block) };
                for v in array.iter() {
                    num_available += v.count_zeros();
                }
            }
        }
        self.num_available_clusters = num_available;
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
            sector_size_shift: boot_sector.bytes_per_sector_shift,
            num_clusters: boot_sector.cluster_count.to_ne(),
            percent_inuse: boot_sector.percent_inuse,
            num_available_clusters: 0,
        };
        bitmap.init().await?;
        Ok(bitmap)
    }

    async fn next_available(&mut self, current: ClusterID) -> Result<(usize, usize), Error<E>> {
        let byte_offset = u32::from(current) / 8;
        let sector_size = 1 << self.sector_size_shift;
        let num_sector = self.length / sector_size;
        let start_sector = byte_offset / sector_size;
        let mut array_offset = (byte_offset % sector_size) as usize / size_of::<usize>();
        for i in 0..num_sector {
            let sector_index = (start_sector + i) % num_sector;
            let sector = self.io.read((self.base + sector_index).id()).await?;
            for (block_index, block) in sector.iter().enumerate() {
                let array: &[usize; 512 / size_of::<usize>()] = unsafe { transmute(block) };
                let array = &array[array_offset..];
                match array.iter().enumerate().find(|(_, &v)| v != usize::MAX) {
                    Some((index, &value)) => {
                        let mut byte_offset = (sector_index * sector_size) as usize;
                        byte_offset += block_index * 512 + index * size_of::<usize>();
                        return Ok((byte_offset, usize::from_be(value)));
                    }
                    None => (),
                }
                array_offset = 0;
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

    pub async fn allocate(&mut self, cluster_id: ClusterID) -> Result<ClusterID, Error<E>> {
        trace!("Allocate cluster starts from {}", cluster_id);
        if self.num_available_clusters >= self.num_clusters {
            return Err(Error::NoSpace);
        }
        let (byte_offset, bits) = self.next_available(cluster_id).await?;
        let bit_offset = bit_offset(first_zero_bit(bits)) as u32;
        let sector_size = 1 << self.sector_size_shift;
        let sector_id = (self.base + (byte_offset / sector_size) as u32).id();
        let offset = byte_offset % sector_size;
        let bytes = (bits | (1 << bit_offset)).to_be_bytes();
        self.io.write(sector_id, offset, &bytes).await?;
        self.num_available_clusters -= 1;
        self.ensure_percent_inuse().await?;
        let cluster_id = ClusterID::from(byte_offset as u32 * 8 + bit_offset);
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
