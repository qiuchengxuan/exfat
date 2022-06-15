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

use cluster_heap::clusters::Clusters;
use cluster_heap::root::RootDirectory as RootDir;
use error::Error;
use fat::FAT;

pub use cluster_heap::directory::FileOrDirectory;

pub struct ExFAT<IO> {
    io: IO,
    serial_number: u32,
    root_directory_cluster_index: u32,
    clusters: Clusters,
}

#[deasync::deasync]
impl<E, IO: io::IO<Error = E>> ExFAT<IO> {
    pub async fn new(mut io: IO) -> Result<Self, Error<E>> {
        let bytes = io.read(0).await.map_err(|e| Error::IO(e))?;
        let sector: &[u8; 512] = bytes.try_into().map_err(|_| Error::EOF)?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(sector) };
        if !boot_sector.is_exfat() {
            return Err(Error::NotExFAT);
        }
        let sector_size = boot_sector.bytes_per_sector();
        let fat_offset = boot_sector.fat_offset.to_ne();
        let fat_length = boot_sector.fat_length.to_ne();

        io.set_sector_size(sector_size).map_err(|e| Error::IO(e))?;
        let clusters = Clusters {
            fat: FAT::new(sector_size, fat_offset, fat_length),
            heap_offset: boot_sector.cluster_heap_offset.to_ne(),
            sectors_per_cluster: boot_sector.sectors_per_cluster(),
            sector_size,
        };
        Ok(Self {
            io,
            serial_number: boot_sector.volumn_serial_number.to_ne(),
            root_directory_cluster_index: boot_sector.first_cluster_of_root_directory.to_ne(),
            clusters,
        })
    }

    pub async fn validate_checksum(&mut self) -> Result<(), Error<E>> {
        let mut checksum = region::boot::BootChecksum::default();
        for i in 0..=10 {
            let bytes = self.io.read(i as u64).await.map_err(|e| Error::IO(e))?;
            checksum.write(i, bytes);
        }
        let bytes = self.io.read(11).await.map_err(|e| Error::IO(e))?;
        let bytes: &[u8; 4] = &bytes[..4].try_into().map_err(|_| Error::EOF)?;
        if u32::from_le_bytes(*bytes) != checksum.sum() {
            return Err(Error::Checksum);
        }
        Ok(())
    }

    pub fn serial_number(&self) -> u32 {
        self.serial_number
    }

    pub async fn root_directory<'a>(&'a mut self) -> Result<RootDir<IO>, Error<E>> {
        let io = self.io.clone();
        let cluster_index = self.root_directory_cluster_index;
        RootDir::open(io, cluster_index, self.clusters).await
    }
}

unsafe impl<IO: Send> Send for ExFAT<IO> {}
