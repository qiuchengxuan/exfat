#![doc = include_str!("../README.md")]
#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(all(feature = "async", feature = "std", not(any(feature = "smol", feature = "tokio"))))]
compile_error!("Either smol or tokio must be selected");

extern crate alloc;

#[macro_use]
extern crate hex_literal;
extern crate heapless;
#[macro_use]
extern crate log;

mod cluster_heap;
mod endian;
pub mod error;
mod fat;
pub mod file;
pub(crate) mod fs;
pub mod io;
mod region;
pub(crate) mod sync;
pub mod types;
mod upcase_table;

use core::fmt::Debug;
use core::mem;

use memoffset::offset_of;

pub use cluster_heap::directory::{Directory, FileOrDirectory};
pub use cluster_heap::file::SeekFrom;
pub use cluster_heap::root::RootDirectory;
use error::{DataError, Error, ImplementationError};
use io::IOWrapper;
pub use region::data::entryset::primary::DateTime;
use sync::{Shared, shared};
use types::ClusterID;

pub struct ExFAT<IO> {
    io: Shared<IOWrapper<IO>>,
    serial_number: u32,
    fat_info: fat::Info,
    fs_info: fs::Info,
    root: ClusterID,
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E: Debug, IO: io::IO<Error = E>> ExFAT<IO> {
    pub async fn new(mut io: IO) -> Result<Self, Error<E>> {
        let blocks = io.read(0.into()).await.map_err(|e| Error::IO(e))?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&blocks[0]) };
        if !boot_sector.is_exfat() {
            return Err(DataError::NotExFAT.into());
        }
        if boot_sector.number_of_fats > 1 {
            return Err(ImplementationError::TexFATNotSupported.into());
        }
        let fat_offset = boot_sector.fat_offset.to_ne();
        let fat_length = boot_sector.fat_length.to_ne();
        debug!("FAT offset {} length {}", fat_offset, fat_length);

        io.set_sector_size_shift(boot_sector.bytes_per_sector_shift).map_err(|e| Error::IO(e))?;
        let root = ClusterID::from(boot_sector.first_cluster_of_root_directory.to_ne());
        debug!("Root directory on cluster {}", root);
        let sector_size_shift = boot_sector.bytes_per_sector_shift;
        let fat_info = fat::Info::new(sector_size_shift, fat_offset, fat_length);
        let fs_info = fs::Info {
            heap_offset: boot_sector.cluster_heap_offset.to_ne(),
            sectors_per_cluster_shift: boot_sector.sectors_per_cluster_shift,
            sector_size_shift,
        };
        debug!("Filesystem info: {:?}", fs_info);
        Ok(Self {
            io: shared(IOWrapper::new(io)),
            serial_number: boot_sector.volumn_serial_number.to_ne(),
            fs_info,
            fat_info,
            root,
        })
    }

    pub async fn is_dirty(&mut self) -> Result<bool, Error<E>> {
        let mut io = acquire!(self.io);
        let blocks = io.read(0.into()).await?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&blocks[0]) };
        Ok(boot_sector.volume_flags().volume_dirty() > 0)
    }

    pub async fn percent_inuse(&mut self) -> Result<u8, Error<E>> {
        let mut io = acquire!(self.io);
        let blocks = io.read(0.into()).await?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&blocks[0]) };
        Ok(boot_sector.percent_inuse)
    }

    pub async fn set_dirty(&mut self, dirty: bool) -> Result<(), Error<E>> {
        let mut io = acquire!(self.io);
        let sector = io.read(0.into()).await?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&sector[0]) };
        let mut volume_flags = boot_sector.volume_flags();
        volume_flags.set_volume_dirty(dirty as u16);
        let offset = offset_of!(region::boot::BootSector, volume_flags);
        let bytes: [u8; 2] = unsafe { mem::transmute(volume_flags) };
        io.write(0.into(), offset, &bytes).await?;
        io.flush().await
    }

    pub async fn validate_checksum(&mut self) -> Result<(), Error<E>> {
        let mut io = acquire!(self.io);
        let mut checksum = region::boot::BootChecksum::default();
        for i in 0..=10 {
            let sector = io.read(i.into()).await?;
            for block in sector.iter() {
                checksum.write(i as usize, block);
            }
        }
        let sector = io.read(11.into()).await?;
        let array: &[u32; 128] = unsafe { core::mem::transmute(&sector[0]) };
        if u32::from_le(array[0]) != checksum.sum() {
            return Err(DataError::BootChecksum.into());
        }
        Ok(())
    }

    pub fn serial_number(&self) -> u32 {
        self.serial_number
    }

    pub async fn root_directory(&mut self) -> Result<RootDirectory<E, IO>, Error<E>> {
        let io = self.io.clone();
        RootDirectory::new(io, self.fat_info, self.fs_info, self.root).await
    }

    pub fn try_free(self) -> Result<IO, Self> {
        let ExFAT { io, serial_number, fat_info, fs_info, root } = self;
        let io = match () {
            #[cfg(all(feature = "sync", any(feature = "tokio", feature = "smol")))]
            () => alloc::sync::Arc::try_unwrap(io).map(|mutex| mutex.into_inner()),
            #[cfg(all(feature = "sync", feature = "std", not(feature = "async")))]
            () => alloc::sync::Arc::try_unwrap(io).map(|mutex| mutex.into_inner().unwrap()),
            #[cfg(not(feature = "sync"))]
            () => alloc::rc::Rc::try_unwrap(io).map(|cell| cell.into_inner()),
        };
        match io {
            Ok(io) => Ok(io.unwrap()),
            Err(io) => Err(Self { io, serial_number, fat_info, fs_info, root }),
        }
    }
}
