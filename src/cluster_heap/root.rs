use core::mem;

#[cfg(not(feature = "std"))]
use alloc::rc::Rc;
#[cfg(feature = "std")]
use std::rc::Rc;

#[cfg(any(feature = "async", feature = "std"))]
use super::allocation_bitmap::AllocationBitmap;
use super::clusters::SectorRef;
use super::directory::Directory;
use super::entry::ClusterEntry;
use crate::endian::Little as LE;
use crate::error::Error;
use crate::fat;
use crate::io::IOWrapper;
use crate::region;
use crate::region::data::entry_type::{EntryType, RawEntryType};
use crate::region::data::entryset::RawEntry;
#[cfg(any(feature = "async", feature = "std"))]
use crate::sync::{Arc, Mutex};

pub struct RootDirectory<IO> {
    directory: Directory<IO>,
    upcase_table: region::data::UpcaseTable,
    volumn_label: Option<heapless::String<11>>,
}

#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> RootDirectory<IO> {
    pub(crate) async fn open(
        mut io: IOWrapper<IO>,
        fat_info: fat::Info,
        sector_ref: SectorRef,
        sector_size_shift: u8,
    ) -> Result<Self, Error<E>> {
        let mut volumn_label: Option<heapless::String<11>> = None;
        let mut upcase_table: Option<region::data::UpcaseTable> = None;
        let mut allocation_bitmap: Option<region::data::AllocationBitmap> = None;
        let sector = io.read(sector_ref.id()).await?;
        let entries: &[RawEntry; 16] = unsafe { mem::transmute(&sector[0]) };
        for entry in entries.iter() {
            match RawEntryType::new(entry[0]).entry_type() {
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
                _ => break,
            };
        }
        let upcase_table = upcase_table.ok_or(Error::UpcaseTableMissing)?;
        #[cfg(any(feature = "async", feature = "std"))]
        let allocation_bitmap = {
            let region = allocation_bitmap.ok_or(Error::AllocationBitmapMissing)?;
            let first = sector_ref.new(region.first_cluster.to_ne().into(), 0);
            let length = region.data_length.to_ne() as u32;
            Arc::new(Mutex::new(
                AllocationBitmap::new(io.clone(), first, length).await?,
            ))
        };
        #[cfg(not(any(feature = "async", feature = "std")))]
        let _ = allocation_bitmap;
        let cluster_id = upcase_table.first_cluster.to_ne();
        let sector = io.read(sector_ref.new(cluster_id.into(), 0).id()).await?;
        let array: &[LE<u16>; 128] = unsafe { mem::transmute(&sector[0]) };
        let directory = Directory {
            entry: ClusterEntry {
                io,
                #[cfg(any(feature = "async", feature = "std"))]
                allocation_bitmap,
                fat_info,
                meta: Default::default(),
                entry_index: 0,
                sector_ref,
                sector_size_shift,
                length: 0,
                capacity: 0,
            },
            upcase_table: Rc::new((*array).into()),
        };
        Ok(Self {
            directory,
            upcase_table,
            volumn_label,
        })
    }

    pub async fn validate_upcase_table_checksum(&mut self) -> Result<(), Error<E>> {
        let mut checksum = region::data::Checksum::default();
        let first_cluster = self.upcase_table.first_cluster.to_ne();
        let sector_ref = &self.directory.entry.sector_ref;
        let first_sector = sector_ref.new(first_cluster.into(), 0).id();
        let data_length = self.upcase_table.data_length.to_ne();
        let sector_size = 1 << self.directory.entry.sector_size_shift;
        let num_sectors = data_length / sector_size as u64;
        let io = &mut self.directory.entry.io;
        for i in 0..num_sectors {
            let sector = io.read(first_sector + i).await?;
            checksum.write(crate::io::flatten(sector));
        }
        let remain = (data_length - num_sectors * sector_size as u64) as usize;
        if remain > 0 {
            let sector_ref = first_sector + num_sectors;
            let sector = io.read(sector_ref).await?;
            checksum.write(&crate::io::flatten(sector)[..remain]);
        }
        if checksum.sum() != self.upcase_table.table_checksum.to_ne() {
            return Err(Error::UpcaseTableChecksum);
        }
        Ok(())
    }

    pub fn volumn_label(&self) -> Option<&str> {
        self.volumn_label.as_ref().map(|label| label.as_str())
    }

    pub fn directory<'a>(&'a mut self) -> &'a mut Directory<IO> {
        &mut self.directory
    }

    pub fn close(self) {}
}

unsafe impl<IO: Send> Send for RootDirectory<IO> {}
