use memoffset::offset_of;

#[cfg(any(feature = "async", feature = "std"))]
use super::allocation_bitmap::AllocationBitmap;
use super::clusters::SectorIndex;
use crate::error::Error;
use crate::fat::FAT;
use crate::io::IOWrapper;
use crate::region::data::entryset::primary::{DateTime, FileDirectory};
use crate::region::data::entryset::secondary::{Secondary, StreamExtension};
use crate::region::data::entryset::{RawEntry, ENTRY_SIZE};
use crate::region::fat::Entry;
#[cfg(any(feature = "async", feature = "std"))]
use crate::sync::{Arc, Mutex};

pub(crate) struct ClusterEntry<IO> {
    pub io: IOWrapper<IO>,
    #[cfg(any(feature = "async", feature = "std"))]
    pub allocation_bitmap: Arc<Mutex<AllocationBitmap<IO>>>,
    pub fat: FAT,
    pub meta: SectorIndex,
    pub entry_index: usize, // Within sector
    pub sector_index: SectorIndex,
    pub sector_size_shift: u8,
    pub length: u64,
    pub capacity: u64,
}

#[derive(Copy, Clone)]
pub struct TouchOption {
    pub access: bool,
    pub modified: bool,
}

impl Default for TouchOption {
    fn default() -> Self {
        Self {
            access: true,
            modified: true,
        }
    }
}

#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> ClusterEntry<IO> {
    pub fn cluster_size_shift(&self) -> u8 {
        self.sector_size_shift + self.meta.sectors_per_cluster_shift
    }

    pub async fn next_cluster(&mut self, index: &mut SectorIndex) -> Result<(), Error<E>> {
        if index.next() {
            return Ok(());
        }
        let offset = self.fat.sector(index.cluster).ok_or(Error::EOF)?;
        let sector = self.io.read(offset as u64).await?;
        match self.fat.next_cluster(sector, index.cluster) {
            Ok(Entry::Next(cluster)) => Ok(index.set_cluster(cluster)),
            _ => Err(Error::EOF),
        }
    }

    pub async fn touch(&mut self, datetime: DateTime, option: TouchOption) -> Result<(), Error<E>> {
        let index = self.entry_index;
        let sector_index = self.meta.sector();
        let sector = self.io.read(sector_index).await?;
        let entries: &[[RawEntry; 16]] = unsafe { core::mem::transmute(sector) };
        let raw_entry = entries[index / 16][index % 16];
        let mut file_directory: FileDirectory = unsafe { core::mem::transmute(raw_entry) };
        if option.access {
            file_directory.update_last_accessed_timestamp(datetime);
        }
        if option.modified {
            file_directory.update_last_modified_timestamp(datetime);
        }
        let offset = index * ENTRY_SIZE;
        self.io.write(sector_index, offset, &raw_entry).await
    }
}

#[cfg(any(feature = "async", feature = "std"))]
#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> ClusterEntry<IO> {
    pub async fn allocate(&mut self, cluster_index: u32) -> Result<Option<u32>, Error<E>> {
        let mut bitmap = match () {
            #[cfg(feature = "async")]
            _ => self.allocation_bitmap.lock().await,
            #[cfg(all(not(feature = "async"), feature = "std"))]
            _ => self.allocation_bitmap.lock().unwrap(),
            #[cfg(not(any(feature = "async", feature = "std")))]
            _ => self.allocation_bitmap,
        };
        let next_cluster_index = match bitmap.allocate(cluster_index).await? {
            Some(index) => index,
            None => return Ok(None),
        };
        self.capacity += 1 << self.cluster_size_shift();

        let sector_index = self.fat.sector(cluster_index).unwrap();
        let offset = self.fat.offset(cluster_index);
        let bytes = u32::to_le_bytes(next_cluster_index);
        self.io.write(sector_index as u64, offset, &bytes).await?;

        let index = self.entry_index + 1;
        let sector_index = self.meta.sector();
        let offset = index * ENTRY_SIZE + offset_of!(Secondary<StreamExtension>, data_length);
        let bytes = u64::to_le_bytes(self.capacity);
        self.io.write(sector_index, offset, &bytes).await?;
        Ok(Some(next_cluster_index))
    }

    pub async fn update_length(&mut self, length: u64) -> Result<(), Error<E>> {
        self.length = length;
        let index = self.entry_index + 1;
        let sector_index = self.meta.sector();
        let offset = index * ENTRY_SIZE
            + offset_of!(Secondary<StreamExtension>, custom_defined)
            + offset_of!(StreamExtension, valid_data_length);
        let bytes = u64::to_le_bytes(self.capacity);
        self.io.write(sector_index, offset, &bytes).await
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
