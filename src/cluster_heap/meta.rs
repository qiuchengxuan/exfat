use alloc::sync::Arc;
use core::mem::transmute;

use super::context::Context;
use super::entryset::EntryID;
use super::metadata::Metadata;
use crate::error::{AllocationError, DataError, Error, OperationError};
use crate::fat;
use crate::file::{FileOptions, TouchOptions};
use crate::fs::{self, SectorRef};
use crate::io::IOWrapper;
use crate::region::data::entryset::primary::DateTime;
use crate::region::data::entryset::{RawEntry, ENTRY_SIZE};
use crate::region::fat::Entry;
use crate::sync::Mutex;
use crate::types::ClusterID;

#[macro_export]
macro_rules! with_context {
    ($context: expr) => {
        match () {
            #[cfg(feature = "async")]
            _ => $context.lock().await,
            #[cfg(not(feature = "async"))]
            _ => $context.lock().unwrap(),
        }
    };
}

pub(crate) use with_context;

#[derive(Clone)]
pub(crate) struct MetaFileDirectory<IO> {
    pub io: IOWrapper<IO>,
    pub context: Arc<Mutex<Context<IO>>>,
    pub fat_info: fat::Info,
    pub fs_info: fs::Info,
    pub metadata: Metadata,
    pub options: FileOptions,
    pub sector_ref: SectorRef,
}

impl<IO> MetaFileDirectory<IO> {
    pub(crate) fn id(&self) -> EntryID {
        let entry_ref = &self.metadata.entry_ref;
        EntryID { sector_id: entry_ref.sector_ref.id(&self.fs_info), index: entry_ref.index }
    }
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E, IO: crate::io::IO<Error = E>> MetaFileDirectory<IO> {
    pub async fn next(&mut self, sector_ref: SectorRef) -> Result<SectorRef, Error<E>> {
        let fat_chain = self.metadata.stream_extension.general_secondary_flags.fat_chain();
        if sector_ref.sector_index != self.fs_info.sectors_per_cluster() {
            return Ok(sector_ref.next(self.fs_info.sectors_per_cluster_shift));
        }
        if !fat_chain {
            let num_clusters = (self.metadata.length() / self.fs_info.cluster_size() as u64) as u32;
            let max_cluster_id = self.sector_ref.cluster_id + num_clusters;
            if sector_ref.cluster_id + 1u32 >= max_cluster_id {
                return Err(OperationError::EOF.into());
            }
            return Ok(sector_ref.next(self.fs_info.sectors_per_cluster_shift));
        }
        let cluster_id = sector_ref.cluster_id;
        let option = self.fat_info.fat_sector_id(cluster_id);
        let sector_id = option.ok_or(Error::Data(DataError::FATChain))?;
        let sector = self.io.read(sector_id).await?;
        match self.fat_info.next_cluster_id(sector, sector_ref.cluster_id) {
            Ok(Entry::Next(cluster_id)) => Ok(SectorRef::new(cluster_id, 0)),
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
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E, IO: crate::io::IO<Error = E>> MetaFileDirectory<IO> {
    pub async fn allocate(&mut self, last: ClusterID) -> Result<ClusterID, Error<E>> {
        trace!("Allocate cluster with last cluster {}", last);
        if !self.metadata.stream_extension.general_secondary_flags.allocation_possible() {
            return Err(AllocationError::NotPossible.into());
        }
        let fragment = !self.options.dont_fragment;
        let mut context = with_context!(self.context);
        let cluster_id = context.allocation_bitmap.allocate(last, fragment).await?;

        let cluster_size = self.fs_info.cluster_size() as u64;
        let metadata = &mut self.metadata;

        let fat_chain = metadata.stream_extension.general_secondary_flags.fat_chain();
        if !last.valid() {
            metadata.stream_extension.first_cluster = u32::from(cluster_id).into();
            metadata.stream_extension.general_secondary_flags.clear_fat_chain();
        } else if last + 1u32 != cluster_id || fat_chain {
            if !fat_chain && metadata.capacity() > cluster_size {
                let first = self.sector_ref.cluster_id;
                for i in 0..(metadata.capacity() / cluster_size - 1) {
                    let cluster_id = first + i as u32;
                    let next = cluster_id + 1u32;
                    let sector_id = self.fat_info.fat_sector_id(cluster_id).unwrap();
                    let bytes = u32::to_le_bytes(next.into());
                    self.io.write(sector_id, self.fat_info.offset(next), &bytes).await?;
                }
                metadata.stream_extension.general_secondary_flags.set_fat_chain();
            }
            let sector_id = self.fat_info.fat_sector_id(last).unwrap();
            let bytes = u32::to_le_bytes(cluster_id.into());
            self.io.write(sector_id, self.fat_info.offset(last), &bytes).await?;
            let bytes = u32::to_ne_bytes(Entry::Last.into());
            self.io.write(sector_id, self.fat_info.offset(cluster_id), &bytes).await?;
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
        if !metadata.entry_ref.sector_ref.cluster_id.valid() {
            // Probably root directory
            return Ok(());
        }
        if metadata.dirty {
            trace!("Flush metadatadata since dirty");
            let mut sector_id = metadata.entry_ref.sector_ref.id(&self.fs_info);
            let bytes: &RawEntry = unsafe { transmute(&metadata.file_directory) };
            let offset = metadata.entry_ref.index as usize * ENTRY_SIZE;
            self.io.write(sector_id, offset, &bytes[..]).await?;
            let mut offset = (metadata.entry_ref.index as usize + 1) * ENTRY_SIZE;
            if offset == self.fs_info.sector_size() as usize {
                offset = 0;
                sector_id += 1u32;
            }
            let bytes: &RawEntry = unsafe { transmute(&metadata.stream_extension) };
            self.io.write(sector_id, offset, &bytes[..]).await?;
            self.io.flush().await?;
            metadata.dirty = false;
        }
        Ok(())
    }

    pub async fn close(&mut self) -> Result<(), Error<E>> {
        self.sync().await?;
        let mut context = with_context!(self.context);
        context.opened_entries.remove(self.id());
        Ok(())
    }
}
