use core::mem::MaybeUninit;
use core::mem::transmute;

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

use super::{BLOCK_SIZE, Block};
use crate::types::SectorID;

#[derive(Debug)]
pub struct FileIO {
    file: fs::File,
    sector_size_shift: u8,
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
        Ok(Self { file, sector_size_shift: 9 })
    }
}

#[cfg_attr(feature = "async", async_trait)]
#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl super::IO for FileIO {
    type Block = heapless::Vec<Block, 8>;
    type Error = std::io::Error;

    fn set_sector_size_shift(&mut self, shift: u8) -> Result<(), Self::Error> {
        self.sector_size_shift = shift;
        Ok(())
    }

    async fn read<'a>(&'a mut self, sector: SectorID) -> Result<Self::Block, Self::Error> {
        let sector_size: usize = 1 << self.sector_size_shift;
        let seek = SeekFrom::Start(u64::from(sector) * sector_size as u64);

        self.file.seek(seek).await?;
        let block = MaybeUninit::<[u8; 4096]>::uninit();
        let mut buffer = unsafe { block.assume_init() };
        self.file.read_exact(&mut buffer[..sector_size]).await?;
        let array: [Block; 8] = unsafe { transmute(block.assume_init()) };
        let mut retval = heapless::Vec::<Block, 8>::from_array(array);
        retval.truncate(sector_size / BLOCK_SIZE);
        Ok(retval)
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
