use super::clusters::SectorRef;
use super::entry::{ClusterEntry, TouchOption};
use crate::error::Error;
use crate::region::data::entryset::primary::DateTime;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

pub struct File<IO> {
    pub(crate) entry: ClusterEntry<IO>,
    pub(crate) sector_ref: SectorRef,
    pub(crate) size: u64,
    pub(crate) cursor: u64,
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E, IO: crate::io::IO<Error = E>> File<IO> {
    pub fn size(&self) -> u64 {
        self.size
    }

    pub async fn touch(&mut self, datetime: DateTime, option: TouchOption) -> Result<(), Error<E>> {
        self.entry.touch(datetime, option).await?;
        self.entry.io.flush().await
    }

    pub async fn read(&mut self, mut buf: &mut [u8]) -> Result<usize, Error<E>> {
        if self.cursor == self.size {
            return Err(Error::EOF);
        }
        if buf.len() > (self.size - self.cursor) as usize {
            buf = &mut buf[..(self.size - self.cursor) as usize];
        }
        let sector_size = 1 << self.entry.sector_size_shift;
        let offset = self.cursor as usize % sector_size;
        let sector_id = self.sector_ref.id();
        let sector_remain = sector_size - offset;
        let sector = self.entry.io.read(sector_id).await?;
        let bytes = crate::io::flatten(sector);
        if buf.len() <= sector_remain {
            buf.copy_from_slice(&bytes[offset..offset + buf.len()]);
            if buf.len() == sector_remain {
                self.sector_ref = self.entry.next(self.sector_ref).await?;
            }
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
        Ok(buf.len())
    }

    pub async fn write(&mut self, bytes: &[u8]) -> Result<usize, Error<E>> {
        if bytes.len() == 0 {
            return Ok(0);
        }
        let mut remain = bytes;
        let sector_id = self.sector_ref.id();
        let sector_size = 1 << self.entry.sector_size_shift;
        let offset = self.cursor as usize % sector_size;
        let sector_remain = sector_size - offset;
        if sector_remain > 0 {
            if bytes.len() <= sector_remain {
                self.entry.io.write(sector_id, offset, bytes).await?;
                self.cursor += bytes.len() as u64;
                return Ok(bytes.len());
            }
            let chunk = &bytes[..sector_remain];
            self.entry.io.write(sector_id, offset, chunk).await?;
            self.cursor += sector_remain as u64;
            remain = &bytes[sector_remain..];
        }
        while remain.len() > 0 {
            if self.cursor < self.entry.capacity {
                self.sector_ref = self.entry.next(self.sector_ref).await?;
            } else {
                let cluster_id = self.entry.allocate().await?;
                self.sector_ref = self.sector_ref.new(cluster_id, 0);
            }
            let sector_id = self.sector_ref.id();
            let length = core::cmp::min(remain.len(), sector_size);
            let chunk = &remain[..length];
            self.entry.io.write(sector_id, 0, chunk).await?;
            remain = &remain[length..];
        }
        self.cursor += (bytes.len() - remain.len()) as u64;
        if self.cursor > self.size {
            self.size = self.cursor;
        }
        Ok(bytes.len() - remain.len())
    }

    pub async fn flush(&mut self) -> Result<(), Error<E>> {
        if self.size != self.entry.length {
            self.entry.length = self.size;
            self.entry.update_length(self.size).await?;
        }
        self.entry.io.flush().await
    }

    pub async fn seek(&mut self, seek_from: SeekFrom) -> Result<u64, Error<E>> {
        let option = match seek_from {
            SeekFrom::Start(cursor) => i64::try_from(cursor).ok(),
            SeekFrom::End(offset) => Some((self.cursor as i64) + offset),
            SeekFrom::Current(offset) => (self.cursor as i64).checked_add(offset),
        };
        let cursor = option.ok_or(Error::InvalidInput("Seek position"))?;
        if cursor < 0 || cursor >= self.size as i64 {
            return Err(Error::InvalidInput("Seek out of range"));
        }
        let cursor = cursor as u64;
        let sector_size = 1 << self.entry.sector_size_shift;
        if cursor > self.cursor {
            let num_sectors = (cursor - self.cursor + sector_size - 1) / sector_size;
            for _ in 0..num_sectors {
                self.sector_ref = self.entry.next(self.sector_ref).await?;
            }
        } else if cursor < self.cursor {
            let num_sectors = (cursor + sector_size - 1) / sector_size;
            self.sector_ref = self.entry.sector_ref;
            for _ in 0..num_sectors {
                self.sector_ref = self.entry.next(self.sector_ref).await?;
            }
        }
        self.cursor = cursor;
        Ok(cursor)
    }

    pub async fn truncate(&mut self, size: u64) -> Result<(), Error<E>> {
        if size > self.size {
            return Err(Error::InvalidInput("Size larger than file size"));
        }
        if self.cursor > size {
            self.cursor = size;
            self.seek(SeekFrom::Start(size)).await?;
        }
        self.size = size;
        self.entry.update_length(size).await
    }

    pub async fn close(self) -> Result<(), Error<E>> {
        self.entry.close().await
    }
}
