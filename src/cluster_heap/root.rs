use core::mem;

#[cfg(not(feature = "std"))]
use alloc::rc::Rc;
#[cfg(feature = "std")]
use std::rc::Rc;

use super::clusters::Clusters;
use super::directory::Directory;
use super::entry::{ClusterEntry, Offset};
use crate::endian::Little as LE;
use crate::error::Error;
use crate::region;
use crate::region::data::entry_type::{EntryType, RawEntryType};

pub struct RootDirectory<IO> {
    directory: Directory<IO>,
    upcase_table: region::data::UpcaseTable,
    volumn_label: Option<heapless::String<11>>,
    allocation_bitmap: region::data::AllocationBitmap,
}

#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> RootDirectory<IO> {
    pub(crate) async fn open(
        mut io: IO,
        cluster_index: u32,
        clusters: Clusters,
    ) -> Result<Self, Error<E>> {
        let mut volumn_label: Option<heapless::String<11>> = None;
        let mut upcase_table: Option<region::data::UpcaseTable> = None;
        let mut allocation_bitmap: Option<region::data::AllocationBitmap> = None;
        let sector_index = clusters.sector_index(cluster_index.into());
        let sector = io.read(sector_index).await.map_err(|e| Error::IO(e))?;
        for chunk in sector.chunks(32) {
            let entry: &[u8; 32] = chunk.try_into().map_err(|_| Error::EOF)?;
            match RawEntryType::new(chunk[0]).entry_type() {
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
        let sector_index = clusters.sector_index(upcase_table.first_cluster.to_ne().into());
        let sector = io.read(sector_index).await.map_err(|e| Error::IO(e))?;
        let array: &[u8; 256] = sector[..256].try_into().map_err(|_| Error::EOF)?;
        let array: &[LE<u16>; 128] = unsafe { mem::transmute(array) };
        let allocation_bitmap = allocation_bitmap.ok_or(Error::AllocationBitmapMissing)?;
        let entry = ClusterEntry {
            io,
            clusters,
            meta_offset: Offset::invalid(),
            cluster_index,
            length: 0,
            capacity: 0,
        };
        let directory = Directory {
            entry,
            upcase_table: Rc::new((*array).into()),
        };
        Ok(Self {
            directory,
            upcase_table,
            volumn_label,
            allocation_bitmap,
        })
    }

    pub async fn validate_upcase_table_checksum(&mut self) -> Result<(), Error<E>> {
        let mut checksum = region::data::Checksum::default();
        let first_cluster = self.upcase_table.first_cluster.to_ne();
        let clusters = self.directory.entry.clusters;
        let first_sector = clusters.sector_index(first_cluster.into());
        let data_length = self.upcase_table.data_length.to_ne();
        let num_sectors = data_length / clusters.sector_size as u64;
        for i in 0..num_sectors {
            let future = self.directory.entry.io.read(first_sector + i);
            let sector = future.await.map_err(|e| Error::IO(e))?;
            checksum.write(sector);
        }
        let remain = (data_length - num_sectors * clusters.sector_size as u64) as usize;
        if remain > 0 {
            let future = self.directory.entry.io.read(first_sector + num_sectors);
            let sector = future.await.map_err(|e| Error::IO(e))?;
            checksum.write(&sector[..remain]);
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
