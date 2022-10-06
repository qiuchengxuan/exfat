#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(feature = "alloc")]
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
#[cfg(any(feature = "async", feature = "std"))]
pub(crate) mod sync;
mod upcase_table;

use core::mem;

use memoffset::offset_of;

use cluster_heap::clusters::SectorIndex;
pub use cluster_heap::directory::FileOrDirectory;
use cluster_heap::root::RootDirectory as RootDir;
use error::Error;
use fat::FAT;
use io::IOWrapper;

pub struct ExFAT<IO> {
    io: IOWrapper<IO>,
    serial_number: u32,
    sector_size_shift: u8,
    fat: FAT,
    sector_index: SectorIndex,
}

#[deasync::deasync]
impl<E, IO: io::IO<Error = E>> ExFAT<IO> {
    pub async fn new(mut io: IO) -> Result<Self, Error<E>> {
        let blocks = io.read(0).await.map_err(|e| Error::IO(e))?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&blocks[0]) };
        if !boot_sector.is_exfat() {
            return Err(Error::NotExFAT);
        }
        if boot_sector.number_of_fats > 1 {
            return Err(Error::TexFATNotSupported);
        }
        let sector_size = boot_sector.bytes_per_sector() as usize;
        let fat_offset = boot_sector.fat_offset.to_ne();
        let fat_length = boot_sector.fat_length.to_ne();

        io.set_sector_size(sector_size).map_err(|e| Error::IO(e))?;
        let sector_index = SectorIndex {
            heap_offset: boot_sector.cluster_heap_offset.to_ne(),
            sectors_per_cluster: boot_sector.sectors_per_cluster(),
            cluster: boot_sector.first_cluster_of_root_directory.to_ne(),
            sector: 0,
        };
        let sector_size_shift = boot_sector.bytes_per_sector_shift;
        let fat = FAT::new(sector_size_shift, fat_offset, fat_length);
        Ok(Self {
            io: IOWrapper::new(io),
            serial_number: boot_sector.volumn_serial_number.to_ne(),
            sector_size_shift,
            fat,
            sector_index,
        })
    }

    pub async fn is_dirty(&mut self) -> Result<bool, Error<E>> {
        let blocks = self.io.read(0).await?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&blocks[0]) };
        Ok(boot_sector.volume_flags().volume_dirty() > 0)
    }

    pub async fn set_dirty(&mut self, dirty: bool) -> Result<(), Error<E>> {
        let sector = self.io.read(0).await?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&sector[0]) };
        let mut volume_flags = boot_sector.volume_flags();
        volume_flags.set_volume_dirty(dirty as u16);
        let offset = offset_of!(region::boot::BootSector, volume_flags);
        let bytes: [u8; 2] = unsafe { mem::transmute(volume_flags) };
        self.io.write(0, offset, &bytes).await?;
        self.io.flush().await
    }

    pub async fn percent_inuse(&mut self) -> Result<u8, Error<E>> {
        let blocks = self.io.read(0).await?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&blocks[0]) };
        Ok(boot_sector.percent_inuse)
    }

    pub async fn validate_checksum(&mut self) -> Result<(), Error<E>> {
        let mut checksum = region::boot::BootChecksum::default();
        for i in 0..=10 {
            let sector = self.io.read(i as u64).await?;
            for block in sector.iter() {
                checksum.write(i, block);
            }
        }
        let sector = self.io.read(11).await?;
        let array: &[u32; 128] = unsafe { core::mem::transmute(&sector[0]) };
        if u32::from_le(array[0]) != checksum.sum() {
            return Err(Error::Checksum);
        }
        Ok(())
    }

    pub fn serial_number(&self) -> u32 {
        self.serial_number
    }

    pub async fn root_directory(&mut self) -> Result<RootDir<IO>, Error<E>> {
        let io = self.io.clone();
        RootDir::open(io, self.fat, self.sector_index, self.sector_size_shift).await
    }
}

unsafe impl<IO: Send> Send for ExFAT<IO> {}
