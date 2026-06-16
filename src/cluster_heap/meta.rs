use core::mem::transmute;
use core::ops::Deref;

use super::context::Context;
use super::entryset::EntryId;
use super::metadata::Metadata;
use crate::cluster_heap::allocation_bitmap::allocate::Extent;
use crate::error::{AllocationError, DataError, Error, OperationError};
use crate::fat;
use crate::file::{FileOptions, TouchOptions};
use crate::fs::{self, SectorIndex};
use crate::io::{self, Block, Wrap};
use crate::region::data::entryset::primary::DateTime;
use crate::region::data::entryset::{ENTRY_SIZE, RawEntry};
use crate::region::fat::Entry;
use crate::sync::{Share, Shared};
use crate::types::Cluster;

struct Locator {
    pub fat: fat::Meta,
    pub cluster: Cluster,
}

impl Locator {
    fn advance(&mut self) {
        self.cluster = self.cluster + 1u32;
    }

    fn sector(&self) -> u64 {
        self.fat.fat_sector(self.cluster).unwrap()
    }
}

pub struct FileSectors<IO> {
    pub io: IO,
    pub fat: fat::Meta,
    pub fs: fs::Meta,
    pub metadata: Metadata,
    pub sector_index: SectorIndex,
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<B: Deref<Target = [Block]>, E, IO, S: Share<Target = IO>> FileSectors<S>
where
    IO: for<'a> io::IO<Block<'a> = B, Error = E>,
{
    pub async fn next(&mut self, sector_index: SectorIndex) -> Result<SectorIndex, Error<E>> {
        let fat_chain = self.metadata.stream_extension.general_secondary_flags.fat_chain();
        if sector_index.index != self.fs.sectors_per_cluster() {
            return Ok(sector_index.next(self.fs.sectors_per_cluster()));
        }
        if !fat_chain {
            let num_clusters = (self.metadata.length() / self.fs.cluster_size() as u64) as u32;
            let max_cluster_id = self.sector_index.cluster + num_clusters;
            if sector_index.cluster + 1u32 >= max_cluster_id {
                return Err(OperationError::EOF.into());
            }
            return Ok(sector_index.next(self.fs.sectors_per_cluster()));
        }
        let cluster_id = sector_index.cluster;
        let sector = self.fat.fat_sector(cluster_id).ok_or(Error::Data(DataError::FATChain))?;
        let mut io = self.io.acquire().await.wrap();
        let blocks = io.read(sector).await?;
        match self.fat.next_cluster_id(&blocks, sector_index.cluster) {
            Ok(Entry::Next(cluster_id)) => Ok(SectorIndex::new(cluster_id, 0)),
            Ok(Entry::Last) => Err(OperationError::EOF.into()),
            _ => Err(DataError::FATChain.into()),
        }
    }
}

pub struct MetaFileDirectory<IO> {
    pub sectors: FileSectors<IO>,
    pub context: Shared<Context<IO>>,
    pub options: FileOptions,
}

impl<IO> MetaFileDirectory<IO> {
    pub(crate) fn id(&self) -> EntryId {
        let entry_index = &self.sectors.metadata.entry_index;
        let sector = entry_index.sector_index.sector(&self.sectors.fs);
        EntryId { sector, index: entry_index.index }
    }
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<B: Deref<Target = [Block]>, E, IO, S: Share<Target = IO>> MetaFileDirectory<S>
where
    IO: for<'a> io::IO<Block<'a> = B, Error = E>,
{
    pub async fn touch(&mut self, datetime: DateTime, opts: TouchOptions) -> Result<(), Error<E>> {
        let metadata = &mut self.sectors.metadata;
        if opts.access {
            metadata.file_directory.update_last_accessed_timestamp(datetime);
        }
        if opts.modified {
            metadata.file_directory.update_last_modified_timestamp(datetime);
        }
        metadata.update_checksum();
        Ok(())
    }

    pub async fn allocate(&mut self, last: Cluster, size: u32) -> Result<Extent, Error<E>> {
        trace!("Allocate clusters starts from {}", last);
        let flags = self.sectors.metadata.stream_extension.general_secondary_flags;
        if !flags.allocation_possible() {
            return Err(AllocationError::NotPossible.into());
        }
        let mut context = self.context.acquire().await;
        let nofrag = self.options.dont_fragment;
        let extent = context.allocator.find(last, size, nofrag).await?;
        let cluster_size = self.sectors.fs.cluster_size() as u64;
        let metadata = &mut self.sectors.metadata;
        if !last.valid() {
            metadata.stream_extension.first_cluster = extent.start.index().into();
        }
        let contiguous = last + 1u32 == extent.start;
        let chained = last.valid() && (flags.fat_chain() || !contiguous);
        metadata.stream_extension.general_secondary_flags.set_fat_chain(chained);
        if chained {
            let mut io = self.sectors.io.acquire().await.wrap();
            if !last.valid() && metadata.capacity() > cluster_size {
                let first = self.sectors.sector_index.cluster;
                let mut locator = Locator { fat: self.sectors.fat, cluster: first };
                for _ in 0..(metadata.capacity() / cluster_size - 1) {
                    let next = locator.cluster + 1u32;
                    let bytes = u32::to_le_bytes(next.into());
                    io.write(locator.sector(), self.sectors.fat.offset(next), &bytes).await?;
                    locator.advance();
                }
            }
            let mut last = last;
            for cluster in u32::from(extent.start)..u32::from(extent.end) {
                let sector = self.sectors.fat.fat_sector(last).unwrap();
                let bytes = u32::to_le_bytes(cluster.into());
                io.write(sector, self.sectors.fat.offset(last), &bytes).await?;
                last = cluster.into();
            }
            let sector = self.sectors.fat.fat_sector(last).unwrap();
            let bytes = u32::to_ne_bytes(Entry::Last.into());
            io.write(sector, self.sectors.fat.offset(last), &bytes).await?;
        }
        if metadata.file_directory.file_attributes().directory() > 0 {
            let length = metadata.length() + cluster_size;
            metadata.stream_extension.custom_defined.valid_data_length = length.into()
        }
        metadata.stream_extension.data_length = (metadata.capacity() + cluster_size).into();
        metadata.update_checksum();
        context.allocator.allocate(extent.clone()).await?;
        Ok(extent)
    }
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<E, IO, S: Share<Target = IO>> MetaFileDirectory<S>
where
    IO: io::Write<Error = E>,
{
    pub async fn sync(&mut self) -> Result<(), Error<E>> {
        let metadata = &mut self.sectors.metadata;
        if !metadata.entry_index.sector_index.cluster.valid() {
            // Probably root directory
            return Ok(());
        }
        if metadata.dirty {
            trace!("Flush dirty metadata");
            let mut sector = metadata.entry_index.sector_index.sector(&self.sectors.fs);
            let bytes: &RawEntry = unsafe { transmute(&metadata.file_directory) };
            let offset = metadata.entry_index.index as usize * ENTRY_SIZE;
            let mut io = self.sectors.io.acquire().await;
            io.write(sector, offset, &bytes[..]).await.map_err(|e| Error::IO(e))?;
            let mut offset = (metadata.entry_index.index as usize + 1) * ENTRY_SIZE;
            if offset == self.sectors.fs.sector_size() as usize {
                offset = 0;
                sector += 1;
            }
            let bytes: &RawEntry = unsafe { transmute(&metadata.stream_extension) };
            io.write(sector, offset, &bytes[..]).await.map_err(|e| Error::IO(e))?;
            io.flush().await.map_err(|e| Error::IO(e))?;
            metadata.dirty = false;
        }
        Ok(())
    }

    pub async fn close(&mut self) -> Result<(), Error<E>> {
        self.sync().await?;
        self.context.acquire().await.opened_entries.remove(self.id());
        Ok(())
    }
}
