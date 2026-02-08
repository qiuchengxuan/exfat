#![doc = include_str!("../README.md")]
#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(all(feature = "async", feature = "std", not(any(feature = "smol", feature = "tokio"))))]
compile_error!("Either smol or tokio must be selected");

extern crate alloc;

#[macro_use]
extern crate hex_literal;
extern crate heapless;
#[cfg(feature = "logging")]
#[macro_use]
extern crate log;

#[cfg(not(feature = "logging"))]
#[macro_use]
mod logging {
    #[macro_export]
    macro_rules! warn {
        ($($arg:tt)*) => { _ = ($($arg)*) };
    }
    #[macro_export]
    macro_rules! debug {
        ($($arg:tt)*) => { _ = ($($arg)*) };
    }
    #[macro_export]
    macro_rules! trace {
        ($($arg:tt)*) => { _ = ($($arg)*) };
    }
}

mod cluster_heap;
mod endian;
pub mod error;
mod fat;
pub mod file;
pub(crate) mod fs;
pub mod io;
mod region;
pub mod sync;
pub mod types;
mod upcase_table;

use core::fmt::Debug;
use core::mem;
use core::ops::Deref;

use memoffset::offset_of;

pub use cluster_heap::directory::{Directory, FileOrDirectory};
pub use cluster_heap::file::SeekFrom;
pub use cluster_heap::root::RootDirectory;
use cluster_heap::root::RootDirectory as Root;
use error::{DataError, Error, ImplementationError};
use io::Wrap;
pub use region::data::entryset::primary::DateTime;
use types::ClusterID;

use crate::io::Block;
use crate::sync::Share;

pub const BOOT_SECTOR: u64 = 0;

pub struct ExFAT<IO> {
    io: IO,
    serial_number: u32,
    fat: fat::Meta,
    fs: fs::Meta,
    root: ClusterID,
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<B: Deref<Target = [Block]>, E: Debug, IO, S: Share<Target = IO>> ExFAT<S>
where
    IO: io::IO<Block<'static> = B, Error = E>,
{
    pub async fn new(io: S) -> Result<Self, Error<E>> {
        let mut wrap_io = io.acquire().await.wrap();
        let block = wrap_io.read(BOOT_SECTOR).await?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&block[0]) };
        if !boot_sector.is_exfat() {
            return Err(DataError::NotExFAT.into());
        }
        if boot_sector.number_of_fats > 1 {
            return Err(ImplementationError::TexFATNotSupported.into());
        }
        let fat_offset = boot_sector.fat_offset.to_ne();
        let fat_length = boot_sector.fat_length.to_ne();
        debug!("FAT offset {} length {}", fat_offset, fat_length);

        wrap_io.set_sector_size_shift(boot_sector.bytes_per_sector_shift)?;
        let root = ClusterID::from(boot_sector.first_cluster_of_root_directory.to_ne());
        debug!("Root directory on cluster {}", root);
        let sector_size_shift = boot_sector.bytes_per_sector_shift;
        let fat = fat::Meta::new(sector_size_shift, fat_offset, fat_length);
        let fs = fs::Meta {
            heap_offset: boot_sector.cluster_heap_offset.to_ne(),
            sectors_per_cluster_shift: boot_sector.sectors_per_cluster_shift,
            sector_size_shift,
        };
        debug!("Filesystem metainfo: {fs:?}");
        drop(wrap_io);
        let serial_number = boot_sector.volumn_serial_number.to_ne();
        Ok(Self { io, serial_number, fs, fat, root })
    }

    pub async fn is_dirty(&mut self) -> Result<bool, Error<E>> {
        let mut io = self.io.acquire().await.wrap();
        let blocks = io.read(BOOT_SECTOR).await?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&blocks[0]) };
        Ok(boot_sector.volume_flags().volume_dirty() > 0)
    }

    pub async fn percent_inuse(&mut self) -> Result<u8, Error<E>> {
        let mut io = self.io.acquire().await.wrap();
        let blocks = io.read(BOOT_SECTOR).await?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&blocks[0]) };
        Ok(boot_sector.percent_inuse)
    }

    pub async fn set_dirty(&mut self, dirty: bool) -> Result<(), Error<E>> {
        let mut io = self.io.acquire().await.wrap();
        let sector = io.read(BOOT_SECTOR).await?;
        let boot_sector: &region::boot::BootSector = unsafe { mem::transmute(&sector[0]) };
        let mut volume_flags = boot_sector.volume_flags();
        volume_flags.set_volume_dirty(dirty as u16);
        let offset = offset_of!(region::boot::BootSector, volume_flags);
        let bytes: [u8; 2] = unsafe { mem::transmute(volume_flags) };
        io.write(BOOT_SECTOR, offset, &bytes).await?;
        io.flush().await
    }

    pub async fn validate_checksum(&mut self) -> Result<(), Error<E>> {
        let mut io = self.io.acquire().await.wrap();
        let mut checksum = region::boot::BootChecksum::default();
        for i in 0..=10 {
            let sector = io.read(i).await?;
            for block in sector.iter() {
                checksum.write(i as usize, block);
            }
        }
        let sector = io.read(11).await?;
        let array: &[u32; 128] = unsafe { core::mem::transmute(&sector[0]) };
        if u32::from_le(array[0]) != checksum.sum() {
            return Err(DataError::BootChecksum.into());
        }
        Ok(())
    }

    pub fn serial_number(&self) -> u32 {
        self.serial_number
    }

    /// Cluster usage is calculated by default, which is inaccurate, therefore you may encounter
    /// false allocation failure when still some clusters available.
    /// For precise cluster usage calculation, you may call `update_usage` which will cost some time.
    pub async fn root_directory(&mut self) -> Result<Root<S>, Error<E>> {
        Root::new(self.io.clone(), self.fat, self.fs, self.root).await
    }

    pub async fn free(self) -> S {
        self.io
    }
}

#[cfg(test)]
mod test {
    #[test_log::test]
    fn test_exfat() {
        use std::cell::RefCell;
        use std::process::Command as CMD;
        use std::rc::Rc;

        use crate::io::std::FileIO;

        log::set_max_level(log::LevelFilter::Trace);

        let args = ["-s", "4194304", "test.img"];
        let output = CMD::new("truncate").args(args).output().unwrap();
        assert!(output.status.success());
        let output = CMD::new("/usr/sbin/mkfs.exfat").args(["test.img"]).output().unwrap();
        assert!(output.status.success());

        let runner = || -> Result<(), Box<dyn std::error::Error>> {
            let io = Rc::new(RefCell::new(FileIO::open("test.img")?));
            let mut exfat = super::ExFAT::new(io)?;
            let mut root = exfat.root_directory()?;
            root.validate_upcase_table_checksum()?;
            root.update_usage()?;
            let mut root_dir = root.open()?;
            root_dir.create("ut.bin", false)?;
            root_dir.find("ut.bin")?.unwrap();
            let entry = root_dir.find("ut.bin")?.unwrap();
            root_dir.delete(&entry)?;
            Ok(root_dir.close()?)
        };
        runner().unwrap();

        CMD::new("rm").args(["-f", "test.img"]).output().unwrap();
    }
}
