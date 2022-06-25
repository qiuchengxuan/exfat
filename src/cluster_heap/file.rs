use super::clusters::ClusterSector;
use super::entry::{ClusterEntry, TouchOption};
use crate::error::Error;
use crate::region::data::entryset::primary::DateTime;

pub struct File<IO> {
    pub(crate) entry: ClusterEntry<IO>,
    pub(crate) cluster_sector: ClusterSector,
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
        let offset = self.offset as usize % self.entry.clusters.sector_size;
        let sector_index = self.entry.clusters.sector_index(self.cluster_sector);
        let io = &mut self.entry.io;
        let sector_remain = self.entry.clusters.sector_size - offset;
        let sector = io.read(sector_index).await.map_err(|e| Error::IO(e))?;
        let bytes = crate::io::flatten(sector);
        if buf.len() <= sector_remain {
            buf.copy_from_slice(&bytes[offset..offset + buf.len()]);
            if buf.len() == sector_remain {
                self.cluster_sector = self.entry.clusters.next(io, self.cluster_sector).await?;
            }
            return Ok(buf.len());
        }
        buf[..sector_remain].copy_from_slice(&bytes[offset..]);
        let mut remain = &mut buf[sector_remain..];
        self.cluster_sector = self.entry.clusters.next(io, self.cluster_sector).await?;
        for _ in 0..remain.len() / self.entry.clusters.sector_size {
            let sector = io.read(sector_index).await.map_err(|e| Error::IO(e))?;
            let bytes = crate::io::flatten(sector);
            remain[..self.entry.clusters.sector_size].copy_from_slice(bytes);
            self.cluster_sector = self.entry.clusters.next(io, self.cluster_sector).await?;
            remain = &mut remain[self.entry.clusters.sector_size..];
        }
        let sector = io.read(sector_index).await.map_err(|e| Error::IO(e))?;
        let bytes = crate::io::flatten(sector);
        remain.copy_from_slice(&bytes[..remain.len()]);
        Ok(buf.len())
    }
}
