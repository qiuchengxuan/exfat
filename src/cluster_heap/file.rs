use core::fmt::Debug;

use super::entry::ClusterEntry;
use crate::error::{Error, InputError, OperationError};
use crate::file::{FileOptions, TouchOptions};
use crate::fs::SectorRef;
use crate::region::data::entryset::primary::DateTime;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

pub struct File<E: Debug, IO: crate::io::IO<Error = E>> {
    pub(crate) entry: ClusterEntry<IO>,
    pub(crate) sector_ref: SectorRef,
    pub(crate) size: u64,
    cursor: u64,
    dirty: bool,
}

impl<E: Debug, IO: crate::io::IO<Error = E>> File<E, IO> {
    pub(crate) fn new(entry: ClusterEntry<IO>, sector_ref: SectorRef) -> Self {
        let size = entry.meta.length();
        Self { entry, sector_ref, size, cursor: 0, dirty: false }
    }

    pub fn change_options(&mut self, f: impl Fn(&mut FileOptions)) {
        f(&mut self.entry.options)
    }
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E: Debug, IO: crate::io::IO<Error = E>> File<E, IO> {
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Change file timestamp, will not take effect immediately untill flush or sync_all called
    pub async fn touch(&mut self, datetime: DateTime, opts: TouchOptions) -> Result<(), Error<E>> {
        self.entry.touch(datetime, opts).await?;
        self.entry.io.flush().await
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
        let sector_size = self.entry.fs_info.sector_size() as usize;
        let offset = self.cursor as usize % sector_size;
        let sector_id = self.sector_ref.id(&self.entry.fs_info);
        let sector_remain = sector_size - offset;
        let sector = self.entry.io.read(sector_id).await?;
        let bytes = crate::io::flatten(sector);
        if buf.len() <= sector_remain {
            buf.copy_from_slice(&bytes[offset..offset + buf.len()]);
            if buf.len() == sector_remain {
                self.sector_ref = self.entry.next(self.sector_ref).await?;
            }
            self.cursor += buf.len() as u64;
            return Ok(buf.len());
        }
        buf[..sector_remain].copy_from_slice(&bytes[offset..]);
        let mut remain = &mut buf[sector_remain..];
        self.sector_ref = self.entry.next(self.sector_ref).await?;
        for _ in 0..remain.len() / sector_size {
            let sector = self.entry.io.read(sector_id).await?;
            let bytes = crate::io::flatten(sector);
            remain[..sector_size].copy_from_slice(bytes);
            self.sector_ref = self.entry.next(self.sector_ref).await?;
            remain = &mut remain[sector_size..];
        }
        let sector = self.entry.io.read(sector_id).await?;
        let bytes = crate::io::flatten(sector);
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
        let sector_size = self.entry.fs_info.sector_size() as usize;
        let offset = self.cursor as usize % sector_size;
        let capacity = self.entry.meta.capacity();
        let sector_remain = if capacity > 0 { sector_size - offset } else { 0 };
        if sector_remain > 0 {
            let length = core::cmp::min(bytes.len(), sector_remain);
            let chunk = &bytes[..length];
            let sector_id = self.sector_ref.id(&self.entry.fs_info);
            self.entry.io.write(sector_id, offset, chunk).await?;
            self.cursor += length as u64;
            self.size = core::cmp::max(self.cursor, self.size);
            return Ok(sector_remain);
        }
        if self.cursor < capacity {
            self.sector_ref = self.entry.next(self.sector_ref).await?;
        } else {
            let cluster_id = self.entry.allocate(self.sector_ref.cluster_id).await?;
            self.sector_ref = SectorRef::new(cluster_id, 0);
        }
        let sector_id = self.sector_ref.id(&self.entry.fs_info);
        let length = core::cmp::min(bytes.len(), sector_size);
        let chunk = &bytes[..length];
        self.entry.io.write(sector_id, 0, chunk).await?;
        self.cursor += length as u64;
        self.size = core::cmp::max(self.cursor, self.size);
        Ok(length)
    }

    pub async fn write_all(&mut self, bytes: &[u8]) -> Result<(), Error<E>> {
        let written = self.write(bytes).await?;
        for chunk in bytes[written..].chunks(self.entry.fs_info.sector_size() as usize) {
            self.write(chunk).await?;
        }
        Ok(())
    }

    /// Flush data write operations
    pub async fn sync_data(&mut self) -> Result<(), Error<E>> {
        if self.dirty {
            self.entry.io.flush().await?;
            self.dirty = false;
        }
        Ok(())
    }

    /// Flush data write operations and metadata changes
    pub async fn sync_all(&mut self) -> Result<(), Error<E>> {
        self.sync_data().await?;
        self.entry.sync().await
    }

    /// Alias of sync_all
    pub async fn flush(&mut self) -> Result<(), Error<E>> {
        self.sync_all().await
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
        let sector_size = self.entry.fs_info.sector_size() as u64;
        let num_sectors = match () {
            _ if cursor > self.cursor => (cursor - self.cursor + sector_size - 1) / sector_size,
            _ if cursor < self.cursor => {
                self.sector_ref = self.entry.sector_ref;
                (cursor + sector_size - 1) / sector_size
            }
            _ => 0,
        };
        for _ in 0..num_sectors {
            self.sector_ref = self.entry.next(self.sector_ref).await?;
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
        self.entry.meta.set_length(size);
        self.size = size;
        Ok(())
    }

    #[cfg(all(feature = "async", not(feature = "std")))]
    /// `no_std` async only which must be explicitly called
    pub async fn close(mut self) -> Result<(), Error<E>> {
        self.flush().await?;
        self.entry.close().await
    }
}

#[cfg(any(not(feature = "async"), feature = "std"))]
impl<E: Debug, IO: crate::io::IO<Error = E>> Drop for File<E, IO> {
    fn drop(&mut self) {
        match () {
            #[cfg(all(feature = "async", not(feature = "std")))]
            () => panic!("Close must be explicit called"),
            #[cfg(all(feature = "async", feature = "std"))]
            () => async_std::task::block_on(async {
                self.flush().await?;
                self.entry.close().await
            })
            .unwrap(),
            #[cfg(not(feature = "async"))]
            () => {
                self.flush().unwrap();
                self.entry.close().unwrap();
            }
        }
    }
}
