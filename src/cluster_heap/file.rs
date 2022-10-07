use super::clusters::SectorIndex;
use super::entry::{ClusterEntry, TouchOption};
use crate::error::Error;
use crate::region::data::entryset::primary::DateTime;

pub struct File<IO> {
    pub(crate) entry: ClusterEntry<IO>,
    pub(crate) sector_index: SectorIndex,
    pub(crate) offset: u64,
}

#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> File<IO> {
    pub async fn touch(&mut self, datetime: DateTime, option: TouchOption) -> Result<(), Error<E>> {
        self.entry.touch(datetime, option).await
    }

    pub async fn read(&mut self, mut buf: &mut [u8]) -> Result<usize, Error<E>> {
        if buf.len() > self.entry.length as usize {
            buf = &mut buf[..self.entry.length as usize];
        }
        let sector_size = 1 << self.entry.sector_size_shift;
        let offset = self.offset as usize % sector_size;
        let sector_index = self.sector_index.sector();
        let sector_remain = sector_size - offset;
        let sector = self.entry.io.read(sector_index).await?;
        let bytes = crate::io::flatten(sector);
        if buf.len() <= sector_remain {
            buf.copy_from_slice(&bytes[offset..offset + buf.len()]);
            if buf.len() == sector_remain {
                self.entry.next_cluster(&mut self.sector_index).await?;
            }
            return Ok(buf.len());
        }
        buf[..sector_remain].copy_from_slice(&bytes[offset..]);
        let mut remain = &mut buf[sector_remain..];
        self.entry.next_cluster(&mut self.sector_index).await?;
        for _ in 0..remain.len() / sector_size {
            let sector = self.entry.io.read(sector_index).await?;
            let bytes = crate::io::flatten(sector);
            remain[..sector_size].copy_from_slice(bytes);
            self.entry.next_cluster(&mut self.sector_index).await?;
            remain = &mut remain[sector_size..];
        }
        let sector = self.entry.io.read(sector_index).await?;
        let bytes = crate::io::flatten(sector);
        remain.copy_from_slice(&bytes[..remain.len()]);
        Ok(buf.len())
    }
}

#[cfg(any(feature = "async", feature = "std"))]
#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> File<IO> {
    pub async fn write(&mut self, bytes: &[u8]) -> Result<usize, Error<E>> {
        if bytes.len() == 0 {
            return Ok(0);
        }
        let mut remain = bytes;
        let sector_offset = self.sector_index.sector();
        let sector_size = 1 << self.entry.sector_size_shift;
        let offset = self.offset as usize % sector_size;
        let sector_remain = sector_size - offset;
        if sector_remain > 0 {
            if bytes.len() <= sector_remain {
                self.entry.io.write(sector_offset, offset, bytes).await?;
                self.offset += bytes.len() as u64;
                return Ok(bytes.len());
            }
            let chunk = &bytes[..sector_remain];
            self.entry.io.write(sector_offset, offset, chunk).await?;
            self.offset += sector_remain as u64;
            remain = &bytes[sector_remain..];
        }
        while remain.len() > 0 {
            if self.offset < self.entry.capacity {
                self.entry.next_cluster(&mut self.sector_index).await?;
            } else {
                let cluster_index = match self.entry.allocate(self.sector_index.cluster).await? {
                    Some(index) => index,
                    None => break,
                };
                self.sector_index = self.sector_index.with_cluster(cluster_index);
            }
            let sector_offset = self.sector_index.sector();
            let length = core::cmp::min(remain.len(), sector_size);
            let chunk = &remain[..length];
            self.entry.io.write(sector_offset, 0, chunk).await?;
            remain = &remain[length..];
        }
        self.offset += (bytes.len() - remain.len()) as u64;
        if self.offset > self.entry.length {
            self.entry.update_length(self.offset).await?;
        }
        Ok(bytes.len() - remain.len())
    }

    pub async fn flush(&mut self) -> Result<(), Error<E>> {
        self.entry.io.flush().await
    }
}
