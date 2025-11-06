use core::fmt::Debug;
use core::mem;
use core::ops::Deref;

use alloc::rc::Rc;
use alloc::vec::Vec;

use super::directory::Directory;
use super::metadata::Metadata;
use super::{
    allocation_bitmap::AllocationBitmap,
    context::{Context, OpenedEntries},
    meta::MetaFileDirectory,
};
use crate::cluster_heap::directory::EntryIter;
use crate::cluster_heap::meta::FileSectors;
use crate::endian::Little as LE;
use crate::error::{DataError, Error, OperationError};
use crate::fat::Meta as FAT;
use crate::file::FileOptions;
use crate::fs::{Meta as FS, SectorIndex};
use crate::io::{self, Block, Wrap};
use crate::region;
use crate::region::data::entry_type::{EntryType, RawEntryType};
use crate::sync::{Share, Shared};
use crate::types::{ClusterID, SectorID};

macro_rules! all_is_some {
    ($($option:expr),+) => ($($option.is_some()) &&+);
}

pub struct RootDirectory<IO> {
    meta: MetaFileDirectory<IO>,
    upcase_table_data: Rc<crate::upcase_table::UpcaseTable>,
    upcase_table: region::data::UpcaseTable,
    volumn_label: Option<heapless::String<22>>,
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<B: Deref<Target = [Block]>, E: Debug, IO, S: Share<Target = IO>> RootDirectory<S>
where
    IO: io::IO<Block<'static> = B, Error = E>,
{
    pub(crate) async fn new(io: S, fat: FAT, fs: FS, id: ClusterID) -> Result<Self, Error<E>> {
        let mut volumn_label: Option<heapless::String<22>> = None;
        let mut upcase_table: Option<region::data::UpcaseTable> = None;
        let mut allocation_bitmap: Option<region::data::AllocationBitmap> = None;
        let sector_index = SectorIndex::new(id, 0);
        let mut metadata = Metadata::new(Default::default());
        metadata.stream_extension.general_secondary_flags.set_fat_chain();
        let mut sectors = FileSectors { io: io.clone(), fat, fs, metadata, sector_index };
        let mut iter = EntryIter::new(&mut sectors).await?;
        while let Some(entry) = iter.next().await? {
            let raw_type = RawEntryType::from(entry[0]);
            match raw_type.entry_type() {
                Ok(EntryType::AllocationBitmap) => {
                    let bitmap: &region::data::AllocationBitmap = unsafe { mem::transmute(entry) };
                    allocation_bitmap = Some(*bitmap)
                }
                Ok(EntryType::VolumnLabel) => {
                    let label: &region::data::VolumnLabel = unsafe { mem::transmute(entry) };
                    volumn_label = Some((*label).into())
                }
                Ok(EntryType::UpcaseTable) => {
                    let table: &region::data::UpcaseTable = unsafe { mem::transmute(entry) };
                    upcase_table = Some(*table)
                }
                _ if raw_type.is_end_of_directory() => break,
                _ => continue,
            };
            if all_is_some!(allocation_bitmap, upcase_table, volumn_label) {
                break;
            }
        }

        let upcase_table = upcase_table.ok_or(DataError::UpcaseTableMissing)?;
        let context = {
            let region = allocation_bitmap.ok_or(DataError::AllocationBitmapMissing)?;
            let first_cluster = region.first_cluster.to_ne();
            let sector_offset = (first_cluster - 2) * fs.sectors_per_cluster();
            let base = SectorID::from((fs.heap_offset + sector_offset) as u64);
            let size = region.data_length.to_ne() as u32;
            debug!("Allocation bitmap found at cluster {} length {}", first_cluster, size);
            let meta = super::allocation_bitmap::Meta::new(io.clone(), size).await?;
            let bitmap = AllocationBitmap::new(io.clone(), base, fat, meta).await;
            Shared::new(Context {
                allocation_bitmap: bitmap,
                opened_entries: OpenedEntries { entries: Vec::with_capacity(4) },
            })
        };
        let cluster_id = upcase_table.first_cluster.to_ne();
        let length = upcase_table.data_length.to_ne();
        debug!("Upcase table found at cluster {} length {}", cluster_id, length);
        let mut io = io.acquire().await.wrap();
        let sector = io.read(SectorIndex::new(cluster_id.into(), 0).id(&fs)).await?;
        let array: &[LE<u16>; 128] = unsafe { mem::transmute(&sector[0]) };
        let options = FileOptions::default();
        let upcase_table_data = Rc::new((*array).into());
        let meta = MetaFileDirectory { sectors, context, options };
        Ok(Self { meta, upcase_table_data, upcase_table, volumn_label })
    }

    /// Traversing allocation bitmap and gather precise usage info
    pub async fn update_usage(&mut self) -> Result<(), Error<E>> {
        self.meta.context.acquire().await.allocation_bitmap.update_usage().await
    }

    pub async fn validate_upcase_table_checksum(&mut self) -> Result<(), Error<E>> {
        let mut checksum = region::data::Checksum::default();
        let first_cluster = self.upcase_table.first_cluster.to_ne();
        let fs = &self.meta.sectors.fs;
        let first_sector = SectorIndex::new(first_cluster.into(), 0).id(&fs);
        let data_length = self.upcase_table.data_length.to_ne();
        let sector_size = fs.sector_size();
        let num_sectors = data_length / sector_size as u64;
        let mut io = self.meta.sectors.io.acquire().await.wrap();
        for i in 0..num_sectors {
            let sector = io.read(first_sector + i).await?;
            checksum.write(crate::io::flatten(&*sector));
        }
        let remain = (data_length - num_sectors * sector_size as u64) as usize;
        if remain > 0 {
            let sector_index = first_sector + num_sectors;
            let sector = io.read(sector_index).await?;
            checksum.write(&crate::io::flatten(&*sector)[..remain]);
        }
        if checksum.sum() != self.upcase_table.table_checksum.to_ne() {
            return Err(DataError::UpcaseTableChecksum.into());
        }
        Ok(())
    }

    pub fn volumn_label(&self) -> Option<&str> {
        self.volumn_label.as_ref().map(|label| label.as_str())
    }

    pub async fn open(&mut self) -> Result<Directory<S>, Error<E>> {
        let sectors = &self.meta.sectors;
        let sectors =
            FileSectors { io: sectors.io.clone(), metadata: sectors.metadata.clone(), ..*sectors };
        let meta = MetaFileDirectory { sectors, context: self.meta.context.clone(), ..self.meta };
        let mut context = self.meta.context.acquire().await;
        if !context.opened_entries.add(meta.id()) {
            return Err(OperationError::AlreadyOpen.into());
        }
        trace!("Open root directory sector {}", meta.sectors.sector_index);
        Ok(Directory::new(meta, self.upcase_table_data.clone()))
    }
}
