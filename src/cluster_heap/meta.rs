use core::mem::transmute;
use core::ops::Deref;

use super::context::Context;
use super::entryset::EntryID;
use super::metadata::Metadata;
use crate::error::{AllocationError, DataError, Error, OperationError};
use crate::fat;
use crate::file::{FileOptions, TouchOptions};
use crate::fs::{self, SectorIndex};
use crate::io::{self, Block, Wrap};
use crate::region::data::entryset::primary::DateTime;
use crate::region::data::entryset::{ENTRY_SIZE, RawEntry};
use crate::region::fat::Entry;
use crate::sync::Shared;
use crate::types::ClusterID;

pub(crate) struct MetaFileDirectory<IO> {
    pub io: Shared<IO>,
    pub context: Shared<Context<IO>>,
    pub fat: fat::Info,
    pub fs: fs::Info,
    pub metadata: Metadata,
    pub options: FileOptions,
    pub sector_index: SectorIndex,
}

impl<IO> Clone for MetaFileDirectory<IO> {
    fn clone(&self) -> Self {
        Self {
            io: self.io.clone(),
            context: self.context.clone(),
            metadata: self.metadata.clone(),
            ..*self
        }
    }
}

impl<IO> MetaFileDirectory<IO> {
    pub(crate) fn id(&self) -> EntryID {
        let entry_index = &self.metadata.entry_index;
        EntryID { sector_id: entry_index.sector_index.id(&self.fs), index: entry_index.index }
    }
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<B: Deref<Target = [Block]>, E, IO: io::IO<Block = B, Error = E>> MetaFileDirectory<IO> {
    pub async fn next(&mut self, sector_index: SectorIndex) -> Result<SectorIndex, Error<E>> {
        let fat_chain = self.metadata.stream_extension.general_secondary_flags.fat_chain();
        if sector_index.sector_index != self.fs.sectors_per_cluster() {
            return Ok(sector_index.next(self.fs.sectors_per_cluster_shift));
        }
        if !fat_chain {
            let num_clusters = (self.metadata.length() / self.fs.cluster_size() as u64) as u32;
            let max_cluster_id = self.sector_index.cluster_id + num_clusters;
            if sector_index.cluster_id + 1u32 >= max_cluster_id {
                return Err(OperationError::EOF.into());
            }
            return Ok(sector_index.next(self.fs.sectors_per_cluster_shift));
        }
        let cluster_id = sector_index.cluster_id;
        let option = self.fat.fat_sector_id(cluster_id);
        let sector_id = option.ok_or(Error::Data(DataError::FATChain))?;
        let mut io = self.io.acquire().await.wrap();
        let block = io.read(sector_id).await?;
        match self.fat.next_cluster_id(&block, sector_index.cluster_id) {
            Ok(Entry::Next(cluster_id)) => Ok(SectorIndex::new(cluster_id, 0)),
            Ok(Entry::Last) => Err(OperationError::EOF.into()),
            _ => Err(DataError::FATChain.into()),
        }
    }

    pub async fn touch(&mut self, datetime: DateTime, opts: TouchOptions) -> Result<(), Error<E>> {
        let metadata = &mut self.metadata;
        if opts.access {
            metadata.file_directory.update_last_accessed_timestamp(datetime);
        }
        if opts.modified {
            metadata.file_directory.update_last_modified_timestamp(datetime);
        }
        metadata.update_checksum();
        Ok(())
    }

    pub async fn allocate(&mut self, last: ClusterID) -> Result<ClusterID, Error<E>> {
        trace!("Allocate cluster with last cluster {}", last);
        if !self.metadata.stream_extension.general_secondary_flags.allocation_possible() {
            return Err(AllocationError::NotPossible.into());
        }
        let nofrag = if self.options.dont_fragment { Some(last) } else { None };
        let mut context = self.context.acquire().await;
        let cluster_id = context.allocation_bitmap.allocate(nofrag).await?;

        let cluster_size = self.fs.cluster_size() as u64;
        let metadata = &mut self.metadata;

        let fat_chain = metadata.stream_extension.general_secondary_flags.fat_chain();
        if !last.valid() {
            metadata.stream_extension.first_cluster = u32::from(cluster_id).into();
            metadata.stream_extension.general_secondary_flags.clear_fat_chain();
        } else if last + 1u32 != cluster_id || fat_chain {
            let mut io = self.io.acquire().await.wrap();
            if !fat_chain && metadata.capacity() > cluster_size {
                let first = self.sector_index.cluster_id;
                for i in 0..(metadata.capacity() / cluster_size - 1) {
                    let cluster_id = first + i as u32;
                    let next = cluster_id + 1u32;
                    let sector_id = self.fat.fat_sector_id(cluster_id).unwrap();
                    let bytes = u32::to_le_bytes(next.into());
                    io.write(sector_id, self.fat.offset(next), &bytes).await?;
                }
                metadata.stream_extension.general_secondary_flags.set_fat_chain();
            }
            let sector_id = self.fat.fat_sector_id(last).unwrap();
            let bytes = u32::to_le_bytes(cluster_id.into());
            io.write(sector_id, self.fat.offset(last), &bytes).await?;
            let bytes = u32::to_ne_bytes(Entry::Last.into());
            io.write(sector_id, self.fat.offset(cluster_id), &bytes).await?;
        }
        if metadata.file_directory.file_attributes().directory() > 0 {
            let length = metadata.length() + cluster_size;
            metadata.stream_extension.custom_defined.valid_data_length = length.into()
        }
        metadata.stream_extension.data_length = (metadata.capacity() + cluster_size).into();
        metadata.update_checksum();
        Ok(cluster_id)
    }

    pub async fn sync(&mut self) -> Result<(), Error<E>> {
        let metadata = &mut self.metadata;
        if !metadata.entry_index.sector_index.cluster_id.valid() {
            // Probably root directory
            return Ok(());
        }
        if metadata.dirty {
            trace!("Flush metadatadata since dirty");
            let mut sector_id = metadata.entry_index.sector_index.id(&self.fs);
            let bytes: &RawEntry = unsafe { transmute(&metadata.file_directory) };
            let offset = metadata.entry_index.index as usize * ENTRY_SIZE;
            let mut io = self.io.acquire().await.wrap();
            io.write(sector_id, offset, &bytes[..]).await?;
            let mut offset = (metadata.entry_index.index as usize + 1) * ENTRY_SIZE;
            if offset == self.fs.sector_size() as usize {
                offset = 0;
                sector_id += 1u32;
            }
            let bytes: &RawEntry = unsafe { transmute(&metadata.stream_extension) };
            io.write(sector_id, offset, &bytes[..]).await?;
            io.flush().await?;
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
