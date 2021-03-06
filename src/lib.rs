#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[macro_use]
extern crate hex_literal;
extern crate heapless;

mod cluster_heap;
mod endian;
pub mod error;
mod fat;
pub mod io;
mod region;
mod upcase_table;

use core::mem;

use cluster_heap::clusters::ClusterSector;
use cluster_heap::root::RootDirectory as RootDir;
use error::Error;
use fat::FAT;

pub use cluster_heap::directory::FileOrDirectory;

pub struct ExFAT<IO> {
    io: IO,
    serial_number: u32,
    fat: FAT,
    cluster_sector: ClusterSector,
}

#[deasync::deasync]
impl<E, IO: io::IO<Error = E>> ExFAT<IO> {
    pub async fn new(mut io: IO) -> Result<Self, Error<E>> {
        let blocks = io.read(0).await.map_err(|e| Error::IO(e))?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&blocks[0]) };
        if !boot_sector.is_exfat() {
            return Err(Error::NotExFAT);
        }
        let sector_size = boot_sector.bytes_per_sector();
        let fat_offset = boot_sector.fat_offset.to_ne();
        let fat_length = boot_sector.fat_length.to_ne();

        io.set_sector_size(sector_size).map_err(|e| Error::IO(e))?;
        let cluster_sector = ClusterSector {
            heap_offset: boot_sector.cluster_heap_offset.to_ne(),
            sectors_per_cluster: boot_sector.sectors_per_cluster(),
            cluster_index: boot_sector.first_cluster_of_root_directory.to_ne(),
            sector_index: 0,
        };
        let fat = FAT::new(sector_size, fat_offset, fat_length);
        Ok(Self {
            io,
            serial_number: boot_sector.volumn_serial_number.to_ne(),
            fat,
            cluster_sector,
        })
    }

    pub async fn validate_checksum(&mut self) -> Result<(), Error<E>> {
        let mut checksum = region::boot::BootChecksum::default();
        for i in 0..=10 {
            let sector = self.io.read(i as u64).await.map_err(|e| Error::IO(e))?;
            for block in sector.iter() {
                checksum.write(i, block);
            }
        }
        let sector = self.io.read(11).await.map_err(|e| Error::IO(e))?;
        let array: &[u32; 128] = unsafe { core::mem::transmute(&sector[0]) };
        if u32::from_le(array[0]) != checksum.sum() {
            return Err(Error::Checksum);
        }
        Ok(())
    }

    pub fn serial_number(&self) -> u32 {
        self.serial_number
    }

    pub async fn root_directory<'a>(&'a mut self) -> Result<RootDir<IO>, Error<E>> {
        let io = self.io.clone();
        RootDir::open(io, self.fat, self.cluster_sector).await
    }
}

unsafe impl<IO: Send> Send for ExFAT<IO> {}
