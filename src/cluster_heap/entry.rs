use alloc::sync::Arc;

use memoffset::offset_of;

use super::clusters::SectorRef;
use super::context::Context;
use super::entryset::EntryID;
use crate::error::Error;
use crate::fat;
use crate::io::IOWrapper;
use crate::region::data::entryset::primary::{DateTime, FileDirectory};
use crate::region::data::entryset::secondary::{Secondary, StreamExtension};
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
            #[cfg(all(not(feature = "async")))]
            _ => $context.lock().unwrap(),
        }
    };
}

pub(crate) use with_context;

pub(crate) fn name_hash(name: &str) -> u16 {
    let mut hash = 0;
    for ch in name.chars() {
        hash = if hash & 1 > 0 { 0x8000 } else { 0 } + (hash >> 1) + ch as u16;
        hash = if hash & 1 > 0 { 0x8000 } else { 0 } + (hash >> 1) + ((ch as u32) >> 16) as u16;
    }
    hash
}

#[derive(Clone)]
pub(crate) struct ClusterEntry<IO> {
    pub io: IOWrapper<IO>,
    pub context: Arc<Mutex<Context<IO>>>,
    pub fat_info: fat::Info,
    pub meta: SectorRef,
    pub entry_index: usize, // Within sector
    pub sector_ref: SectorRef,
    pub sector_size_shift: u8,
    pub last_cluster_id: ClusterID,
    pub length: u64,
    pub capacity: u64,
    pub closed: bool,
}

impl<IO> ClusterEntry<IO> {
    pub(crate) fn id(&self) -> EntryID {
        EntryID { sector_id: self.sector_ref.id(), index: self.entry_index }
    }
}

#[derive(Copy, Clone)]
pub struct TouchOption {
    pub access: bool,
    pub modified: bool,
}

impl Default for TouchOption {
    fn default() -> Self {
        Self { access: true, modified: true }
    }
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E, IO: crate::io::IO<Error = E>> ClusterEntry<IO> {
    pub fn cluster_size_shift(&self) -> u8 {
        self.sector_size_shift + self.meta.sectors_per_cluster_shift
    }

    pub async fn next(&mut self, sector_ref: SectorRef) -> Result<SectorRef, Error<E>> {
        if let Some(sector_ref) = sector_ref.next() {
            return Ok(sector_ref);
        }
        let cluster_id = sector_ref.cluster_id;
        let sector_id = self.fat_info.fat_sector_id(cluster_id).ok_or(Error::EOF)?;
        let sector = self.io.read(sector_id).await?;
        match self.fat_info.next_cluster_id(sector, sector_ref.cluster_id) {
            Ok(Entry::Next(cluster_id)) => Ok(sector_ref.new(cluster_id, 0)),
            _ => Err(Error::EOF),
        }
    }

    pub async fn touch(&mut self, datetime: DateTime, option: TouchOption) -> Result<(), Error<E>> {
        let index = self.entry_index;
        let sector_id = self.meta.id();
        let sector = self.io.read(sector_id).await?;
        let entries: &[[RawEntry; 16]] = unsafe { core::mem::transmute(sector) };
        let mut raw_entry = entries[index / 16][index % 16];
        let file_directory: &mut FileDirectory = unsafe { core::mem::transmute(&mut raw_entry) };
        if option.access {
            file_directory.update_last_accessed_timestamp(datetime);
        }
        if option.modified {
            file_directory.update_last_modified_timestamp(datetime);
        }
        let offset = index * ENTRY_SIZE;
        self.io.write(sector_id, offset, &raw_entry).await
    }
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E, IO: crate::io::IO<Error = E>> ClusterEntry<IO> {
    pub async fn last_cluster_id(&mut self) -> Result<ClusterID, Error<E>> {
        if self.last_cluster_id.valid() {
            return Ok(self.last_cluster_id);
        }
        let mut cluster_id = self.sector_ref.cluster_id;
        if !cluster_id.valid() {
            return Ok(cluster_id);
        }
        let mut sector_id = self.fat_info.fat_sector_id(cluster_id).unwrap();
        let mut sector = self.io.read(sector_id).await?;
        loop {
            match self.fat_info.next_cluster_id(sector, cluster_id).unwrap() {
                Entry::BadCluster => return Err(Error::BadCluster(cluster_id)),
                Entry::Next(id) => cluster_id = id,
                Entry::Last => break,
            }
            let id = self.fat_info.fat_sector_id(cluster_id).unwrap();
            if id != sector_id {
                sector_id = id;
                sector = self.io.read(sector_id).await?;
            }
        }
        self.last_cluster_id = cluster_id;
        Ok(cluster_id)
    }

    pub async fn allocate(&mut self) -> Result<ClusterID, Error<E>> {
        let last_cluster_id = self.last_cluster_id().await?;
        let mut context = with_context!(self.context);
        let cluster_id = context.allocation_bitmap.allocate(last_cluster_id).await?;
        self.capacity += 1 << self.cluster_size_shift();

        let sector_id = self.fat_info.fat_sector_id(cluster_id).unwrap();
        let offset = self.fat_info.offset(cluster_id);
        let bytes = u32::to_le_bytes(cluster_id.into());
        self.io.write(sector_id, offset, &bytes).await?;

        let index = self.entry_index + 1;
        let sector_id = self.meta.id();
        // TODO: ensure NotFatChain false
        let offset = index * ENTRY_SIZE + offset_of!(Secondary<StreamExtension>, data_length);
        let bytes = u64::to_le_bytes(self.capacity);
        self.io.write(sector_id, offset, &bytes).await?;
        self.io.flush().await?;
        Ok(cluster_id)
    }

    pub async fn update_length(&mut self, length: u64) -> Result<(), Error<E>> {
        self.length = length;
        let index = self.entry_index + 1;
        let sector_id = self.meta.id();
        let offset = index * ENTRY_SIZE
            + offset_of!(Secondary<StreamExtension>, custom_defined)
            + offset_of!(StreamExtension, valid_data_length);
        let bytes = u64::to_le_bytes(self.length);
        self.io.write(sector_id, offset, &bytes).await?;
        self.io.flush().await
    }

    pub async fn close(mut self) -> Result<(), Error<E>> {
        self.io.flush().await?;
        let mut context = with_context!(self.context);
        context.opened_entries.remove(self.id());
        self.closed = true;
        Ok(())
    }
}

impl<IO> Drop for ClusterEntry<IO> {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        match () {
            #[cfg(feature = "async")]
            () => {
                panic!("Close must be called explicitly");
            }
            #[cfg(not(feature = "async"))]
            () => {
                let mut context = self.context.lock().unwrap();
                context.opened_entries.remove(self.id());
            }
        }
    }
}

#[cfg(test)]
mod test {
    use memoffset::offset_of;

    use crate::region::data::entryset::secondary::{Secondary, StreamExtension};

    #[test]
    fn test_valid_data_length_offset() {
        let offset = offset_of!(Secondary<StreamExtension>, custom_defined)
            + offset_of!(StreamExtension, valid_data_length);
        assert_eq!(offset, 8);
    }
}
