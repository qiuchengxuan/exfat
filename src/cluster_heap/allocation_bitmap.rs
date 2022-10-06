use core::mem::{size_of, transmute};
use core::slice::from_ref;

use memoffset::offset_of;

use super::clusters::SectorIndex;
use crate::error::Error;
use crate::io::IOWrapper;
use crate::region::boot::BootSector;

#[derive(Clone)]
pub(crate) struct AllocationBitmap<IO> {
    io: IOWrapper<IO>,
    base: SectorIndex,
    length: u32,
    sector_size_shift: u8,
    num_clusters: u32,
    percent_inuse: u8,
    num_available_clusters: u32,
}

#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> AllocationBitmap<IO> {
    async fn scan_available_clusters(&mut self) -> Result<u32, Error<E>> {
        let mut num_available = 0;
        for i in 0..self.length / (1 << self.sector_size_shift) {
            let sector = self.io.read((self.base + i).sector()).await?;
            for block in sector.iter() {
                let array: &[usize; 512 / size_of::<usize>()] = unsafe { transmute(block) };
                for v in array.iter() {
                    num_available += v.count_zeros();
                }
            }
        }
        Ok(num_available)
    }

    pub async fn new(
        mut io: IOWrapper<IO>,
        base: SectorIndex,
        length: u32,
    ) -> Result<Self, Error<E>> {
        let blocks = io.read(0).await?;
        let boot_sector: &BootSector = unsafe { transmute(&blocks[0]) };
        let mut bitmap = Self {
            io,
            base,
            length,
            sector_size_shift: boot_sector.bytes_per_sector_shift,
            num_clusters: boot_sector.cluster_count.to_ne(),
            percent_inuse: boot_sector.percent_inuse,
            num_available_clusters: 0,
        };
        bitmap.num_available_clusters = bitmap.scan_available_clusters().await?;
        Ok(bitmap)
    }

    async fn next_available_offset(&mut self, cluster_index: u32) -> Result<usize, Error<E>> {
        let sector_size = 1 << self.sector_size_shift;
        let num_sector = self.length / sector_size;
        let start_sector = cluster_index / 8 / sector_size;
        let mut offset = (cluster_index / 8 % sector_size) as usize / size_of::<usize>();
        for i in 0..num_sector {
            let sector_index = (start_sector + i) % num_sector;
            let sector = self.io.read((self.base + sector_index).sector()).await?;
            for (block_index, block) in sector.iter().enumerate() {
                let array: &[usize; 512 / size_of::<usize>()] = unsafe { transmute(block) };
                let array = &array[offset..];
                match array.iter().enumerate().find(|(_, &v)| v != usize::MAX) {
                    Some((index, _)) => {
                        let offset = (sector_index * sector_size) as usize;
                        return Ok(offset + block_index * 512 + index * size_of::<usize>());
                    }
                    None => (),
                }
                offset = 0;
            }
        }
        Err(Error::Generic("No available allocation bit"))
    }

    async fn check_update_percent_inuse(&mut self) -> Result<(), Error<E>> {
        let percent_inuse = self.num_available_clusters / 1024 * 100 / self.num_clusters;
        if percent_inuse as u8 == self.percent_inuse {
            return Ok(());
        }
        self.percent_inuse = percent_inuse as u8;
        let offset = offset_of!(BootSector, percent_inuse);
        let bytes: [u8; 1] = [self.percent_inuse];
        self.io.write(0, offset, &bytes).await
    }

    pub async fn allocate(&mut self, cluster_index: u32) -> Result<Option<u32>, Error<E>> {
        if self.num_available_clusters >= self.num_clusters {
            return Ok(None);
        }
        let sector_size = 1 << self.sector_size_shift;
        let offset = self.next_available_offset(cluster_index).await?;
        let sector_index = (offset / sector_size) as u32;
        let sector = self.io.read((self.base + sector_index).sector()).await?;
        let chunk = &sector[(offset / 512) % sector.len()][offset..offset + size_of::<usize>()];
        let mut cluster_index = offset * size_of::<u8>();
        let mut byte = 0;
        for i in 0..size_of::<usize>() {
            byte = chunk[i % size_of::<u8>()];
            if byte & (1 << i) == 0 {
                cluster_index += i;
                byte = byte | 1 << i;
                break;
            }
        }
        let sector_offset = (self.base + sector_index).sector();
        let offset = cluster_index / 8 % sector_size;
        let bytes = from_ref(&byte);
        self.io.write(sector_offset, offset, bytes).await?;
        self.check_update_percent_inuse().await?;
        Ok(Some(cluster_index as u32))
    }
}
