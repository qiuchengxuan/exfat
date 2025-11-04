use std::io::Error;
use std::io::SeekFrom;
use std::path::Path;
use std::ptr::from_mut;
use std::slice::from_raw_parts_mut;
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

use super::{BLOCK_SIZE, Block};
use crate::types::SectorID;

#[derive(Debug)]
pub struct FileIO {
    file: fs::File,
    sector_size_shift: u8,
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl FileIO {
    pub async fn open<P: AsRef<Path>>(filepath: P) -> std::io::Result<Self> {
        let mut options = match () {
            #[cfg(feature = "async")]
            () => fs::OpenOptions::new(),
            #[cfg(not(feature = "async"))]
            () => fs::File::options(),
        };
        let file = options.read(true).write(true).open(filepath).await?;
        Ok(Self { file, sector_size_shift: 9 })
    }
}

#[cfg_attr(feature = "async", async_trait)]
#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl super::IO for FileIO {
    type Block<'a> = heapless::Vec<Block, 8>;
    type Error = std::io::Error;

    fn set_sector_size_shift(&mut self, shift: u8) -> Result<(), Self::Error> {
        self.sector_size_shift = shift;
        Ok(())
    }

    async fn read<'a>(&mut self, sector: SectorID) -> Result<Self::Block<'a>, Self::Error> {
        let sector_size: usize = 1 << self.sector_size_shift;
        let seek = SeekFrom::Start(u64::from(sector) * sector_size as u64);

        self.file.seek(seek).await?;
        let mut block = heapless::Vec::<Block, 8>::new();
        unsafe { block.set_len(sector_size / BLOCK_SIZE) };
        let buf: &mut [u8] = unsafe { from_raw_parts_mut(from_mut(&mut block[0][0]), sector_size) };
        self.file.read_exact(buf).await?;
        Ok(block)
    }

    async fn write(&mut self, sector: SectorID, offset: usize, buf: &[u8]) -> Result<(), Error> {
        let sector_size = 1 << self.sector_size_shift;
        let seek = SeekFrom::Start(u64::from(sector) * sector_size + offset as u64);
        self.file.seek(seek).await?;
        self.file.write_all(buf).await.map(|_| ())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.file.flush().await
    }
}
