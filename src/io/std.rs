use std::mem::MaybeUninit;
use std::slice::from_raw_parts;

use std::io::SeekFrom;
use std::path::Path;
#[cfg(not(feature = "async"))]
use std::{fs, io::prelude::*};

#[cfg(all(feature = "async", feature = "smol"))]
use smol::fs;
#[cfg(all(feature = "async", feature = "smol"))]
use smol::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
#[cfg(all(feature = "async", feature = "tokio"))]
use tokio::fs;
#[cfg(all(feature = "async", feature = "tokio"))]
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

#[cfg(feature = "async")]
use async_trait::async_trait;

use crate::types::SectorID;

const MAX_SECTOR_SIZE: usize = 4096;

#[derive(Debug)]
pub struct FileIO {
    file: fs::File,
    sector_size_shift: u8,
    buffer: MaybeUninit<[u8; MAX_SECTOR_SIZE]>,
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl FileIO {
    pub async fn open<P: AsRef<Path>>(filepath: P) -> std::io::Result<Self> {
        let mut options = match () {
            #[cfg(feature = "async")]
            () => fs::OpenOptions::new(),
            #[cfg(not(feature = "async"))]
            () => fs::File::options(),
        };
        let file = options.read(true).write(true).open(filepath).await?;
        Ok(Self { file, sector_size_shift: 9, buffer: MaybeUninit::uninit() })
    }
}

#[cfg_attr(feature = "async", async_trait)]
#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl super::IO for FileIO {
    type Error = std::io::Error;

    fn set_sector_size_shift(&mut self, shift: u8) -> Result<(), Self::Error> {
        self.sector_size_shift = shift;
        Ok(())
    }

    async fn read<'a>(&'a mut self, sector: SectorID) -> Result<&'a [[u8; 512]], Self::Error> {
        let sector_size: usize = 1 << self.sector_size_shift;
        let seek = SeekFrom::Start(u64::from(sector) * sector_size as u64);

        self.file.seek(seek).await?;
        let buffer = unsafe { self.buffer.assume_init_mut() };
        self.file.read_exact(&mut buffer[..sector_size]).await?;

        Ok(unsafe { from_raw_parts(buffer.as_ptr() as *const _, sector_size / 512) })
    }

    async fn write(
        &mut self,
        sector: SectorID,
        offset: usize,
        buf: &[u8],
    ) -> Result<(), Self::Error> {
        let sector_size = 1 << self.sector_size_shift;
        let seek = SeekFrom::Start(u64::from(sector) * sector_size + offset as u64);
        self.file.seek(seek).await?;
        self.file.write_all(buf).await.map(|_| ())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.file.flush().await
    }
}
