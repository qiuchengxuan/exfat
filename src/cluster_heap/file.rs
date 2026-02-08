use core::fmt::Debug;
use core::ops::Deref;

use super::meta::MetaFileDirectory;
use crate::error::{Error, InputError, OperationError};
use crate::file::{FileOptions, TouchOptions};
use crate::fs::SectorIndex;
use crate::io::{self, Block, Wrap};
use crate::region::data::entryset::primary::DateTime;
use crate::sync::Share;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

pub struct File<E: Debug, IO: io::Write<Error = E>, S: Share<Target = IO>> {
    pub(crate) meta: MetaFileDirectory<S>,
    pub(crate) sector_index: SectorIndex,
    pub(crate) size: u64,
    cursor: u64,
    dirty: bool,
    closed: bool,
}

impl<B: Deref<Target = [Block]>, E: Debug, IO, S: Share<Target = IO>> File<E, IO, S>
where
    IO: io::IO<Block<'static> = B, Error = E>,
{
    pub(crate) fn new(meta: MetaFileDirectory<S>, sector_index: SectorIndex) -> Self {
        let size = meta.sectors.metadata.length();
        Self { meta, sector_index, size, cursor: 0, dirty: false, closed: false }
    }

    pub fn change_options(&mut self, f: impl Fn(&mut FileOptions)) {
        f(&mut self.meta.options)
    }
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<E: Debug, IO: io::Write<Error = E>, S: Share<Target = IO>> File<E, IO, S> {
    /// Flush data write operations
    pub async fn sync_data(&mut self) -> Result<(), Error<E>> {
        if self.dirty {
            self.meta.sectors.io.acquire().await.flush().await.map_err(|e| Error::IO(e))?;
            self.dirty = false;
        }
        Ok(())
    }

    /// Flush data write operations and metadata changes
    pub async fn sync_all(&mut self) -> Result<(), Error<E>> {
        self.sync_data().await?;
        self.meta.sync().await
    }

    /// Alias of sync_all
    pub async fn flush(&mut self) -> Result<(), Error<E>> {
        self.sync_all().await
    }

    /// Must be explicitly called in async senario
    pub async fn close(mut self) -> Result<(), Error<E>> {
        self.closed = true;
        self.flush().await.and(self.meta.close().await)
    }
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<B: Deref<Target = [Block]>, E: Debug, IO, S: Share<Target = IO>> File<E, IO, S>
where
    IO: io::IO<Block<'static> = B, Error = E>,
{
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Change file timestamp, will not take effect immediately untill flush or sync_all called
    pub async fn touch(&mut self, datetime: DateTime, opts: TouchOptions) -> Result<(), Error<E>> {
        self.meta.touch(datetime, opts).await?;
        self.meta.sectors.io.acquire().await.wrap().flush().await
    }

    /// Read some bytes
    /// If sector remain bytes fits in buf,
    /// all remain bytes will be read,
    /// Otherwise a sector size or a buf size will be read.
    pub async fn read(&mut self, mut buf: &mut [u8]) -> Result<usize, Error<E>> {
        if self.cursor == self.size {
            return Err(OperationError::EOF.into());
        }
        if buf.len() > (self.size - self.cursor) as usize {
            buf = &mut buf[..(self.size - self.cursor) as usize];
        }
        let sector_size = self.meta.sectors.fs.sector_size() as usize;
        let offset = self.cursor as usize % sector_size;
        let sector = self.sector_index.sector(&self.meta.sectors.fs);
        let sector_remain = sector_size - offset;
        let mut io = self.meta.sectors.io.acquire().await.wrap();
        let blocks = io.read(sector).await?;
        let bytes = crate::io::flatten(&blocks);
        if buf.len() <= sector_remain {
            buf.copy_from_slice(&bytes[offset..offset + buf.len()]);
            drop(io);
            if buf.len() == sector_remain {
                self.sector_index = self.meta.sectors.next(self.sector_index).await?;
            }
            self.cursor += buf.len() as u64;
            return Ok(buf.len());
        }
        buf[..sector_remain].copy_from_slice(&bytes[offset..]);
        drop(io);
        let mut remain = &mut buf[sector_remain..];
        self.sector_index = self.meta.sectors.next(self.sector_index).await?;
        for _ in 0..remain.len() / sector_size {
            let mut io = self.meta.sectors.io.acquire().await.wrap();
            let blocks = io.read(sector).await?;
            let bytes = crate::io::flatten(&blocks);
            remain[..sector_size].copy_from_slice(bytes);
            drop(io);
            self.sector_index = self.meta.sectors.next(self.sector_index).await?;
            remain = &mut remain[sector_size..];
        }
        let mut io = self.meta.sectors.io.acquire().await.wrap();
        let blocks = io.read(sector).await?;
        let bytes = crate::io::flatten(&blocks);
        remain.copy_from_slice(&bytes[..remain.len()]);
        self.cursor += buf.len() as u64;
        Ok(buf.len())
    }

    /// Write some bytes
    /// If bytes length fits in current sector remain size,
    /// all bytes will be successfully written,
    /// Otherwise a sector size will be written.
    ///
    /// Write operation will not apply file metadata change immediately until
    /// flush or sync_all called.
    pub async fn write(&mut self, bytes: &[u8]) -> Result<usize, Error<E>> {
        if bytes.len() == 0 {
            return Ok(0);
        }
        self.dirty = true;
        let sector_size = self.meta.sectors.fs.sector_size() as usize;
        let mut capacity = self.meta.sectors.metadata.capacity();
        let sector_remain = (capacity - self.cursor) as usize % sector_size;
        if sector_remain > 0 {
            let length = core::cmp::min(bytes.len(), sector_remain);
            let chunk = &bytes[..length];
            trace!("Write to sector-ref {}", self.sector_index);
            let sector = self.sector_index.sector(&self.meta.sectors.fs);
            let mut io = self.meta.sectors.io.acquire().await.wrap();
            io.write(sector, self.cursor as usize % sector_size, chunk).await?;
            drop(io);
            self.cursor += length as u64;
            self.size = core::cmp::max(self.cursor, self.size);
            if length == sector_remain && self.cursor < capacity {
                self.sector_index = self.meta.sectors.next(self.sector_index).await?;
            }
            return Ok(sector_remain);
        }
        if self.cursor >= capacity {
            let cluster = self.meta.allocate(self.sector_index.cluster, 1).await?.base;
            self.sector_index = SectorIndex::new(cluster, 0);
            capacity = self.meta.sectors.metadata.capacity();
        }
        trace!("Write to sector-ref {}", self.sector_index);
        let sector = self.sector_index.sector(&self.meta.sectors.fs);
        let length = core::cmp::min(bytes.len(), sector_size);
        let chunk = &bytes[..length];
        self.meta.sectors.io.acquire().await.wrap().write(sector, 0, chunk).await?;
        self.cursor += length as u64;
        self.size = core::cmp::max(self.cursor, self.size);
        if length == sector_size && self.cursor < capacity {
            self.sector_index = self.meta.sectors.next(self.sector_index).await?;
        }
        self.meta.sectors.metadata.set_length(self.size);
        Ok(length)
    }

    pub async fn write_all(&mut self, bytes: &[u8]) -> Result<(), Error<E>> {
        let written = self.write(bytes).await?; // Fill remain of current sector
        for chunk in bytes[written..].chunks(self.meta.sectors.fs.sector_size() as usize) {
            self.write(chunk).await?;
        }
        Ok(())
    }

    /// Change current cursor position
    pub async fn seek(&mut self, seek_from: SeekFrom) -> Result<u64, Error<E>> {
        let option = match seek_from {
            SeekFrom::Start(cursor) => i64::try_from(cursor).ok(),
            SeekFrom::End(offset) => Some((self.cursor as i64) + offset),
            SeekFrom::Current(offset) => (self.cursor as i64).checked_add(offset),
        };
        let cursor = option.ok_or(Error::Input(InputError::SeekPosition))?;
        if cursor < 0 || cursor >= self.size as i64 {
            return Err(InputError::SeekPosition.into());
        }
        let cursor = cursor as u64;
        let sector_size = self.meta.sectors.fs.sector_size() as u64;
        let num_sectors = match () {
            _ if cursor > self.cursor => (cursor - self.cursor + sector_size - 1) / sector_size,
            _ if cursor < self.cursor => {
                self.sector_index = self.meta.sectors.sector_index;
                (cursor + sector_size - 1) / sector_size
            }
            _ => 0,
        };
        for _ in 0..num_sectors {
            self.sector_index = self.meta.sectors.next(self.sector_index).await?;
        }
        self.cursor = cursor;
        Ok(cursor)
    }

    /// Shrink current file size
    pub async fn truncate(&mut self, size: u64) -> Result<(), Error<E>> {
        if size > self.size {
            return Err(InputError::Size.into());
        }
        if self.cursor > size {
            self.cursor = size;
            self.seek(SeekFrom::Start(size)).await?;
        }
        self.meta.sectors.metadata.set_length(size);
        self.size = size;
        Ok(())
    }
}

impl<E: Debug, IO: io::Write<Error = E>, S: Share<Target = IO>> Drop for File<E, IO, S> {
    #[cfg(not(feature = "async"))]
    fn drop(&mut self) {
        self.flush().and(self.meta.close()).unwrap()
    }
    #[cfg(feature = "async")]
    fn drop(&mut self) {
        if cfg!(debug_assertions) && !self.closed {
            panic!("File at {} not closed", self.meta.sectors.sector_index);
        }
    }
}
