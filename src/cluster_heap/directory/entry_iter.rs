use core::fmt::Debug;
use core::mem;

use super::super::meta::MetaFileDirectory;
use crate::error::Error;
use crate::fs::SectorIndex;
use crate::region::data::entry_type::RawEntryType;
use crate::region::data::entryset::{ENTRY_SIZE, RawEntry};
use crate::sync::acquire;

pub(crate) struct EntryIter<'a, IO> {
    meta: &'a mut MetaFileDirectory<IO>,
    entries: &'a [[RawEntry; 16]],
    pub sector_index: SectorIndex,
    pub index: u8,
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<'a, E: Debug, IO: crate::io::IO<Error = E>> EntryIter<'a, IO> {
    pub(crate) async fn new(
        meta: &'a mut MetaFileDirectory<IO>,
    ) -> Result<EntryIter<'a, IO>, Error<E>> {
        let sector_index = meta.sector_index;
        let mut io = acquire!(meta.io);
        let sector = io.read(sector_index.id(&meta.fs_info)).await?;
        let entries = unsafe { mem::transmute(sector) };
        drop(io);
        Ok(Self { meta, entries, sector_index, index: u8::MAX })
    }

    pub(crate) async fn skip(&mut self, num_entries: u8) -> Result<(), Error<E>> {
        self.index = self.index.wrapping_add(num_entries);
        let sector_size = self.meta.fs_info.sector_size() as usize;
        if (self.index as usize * ENTRY_SIZE) >= sector_size {
            self.index -= (sector_size / ENTRY_SIZE) as u8;
            self.sector_index = self.meta.next(self.sector_index).await?;
            let mut io = acquire!(self.meta.io);
            let sector = io.read(self.sector_index.id(&self.meta.fs_info)).await?;
            self.entries = unsafe { mem::transmute(sector) };
        }
        Ok(())
    }

    pub async fn next(&mut self) -> Result<Option<&'a RawEntry>, Error<E>> {
        self.skip(1).await?;
        let index = self.index as usize;
        let entry = &self.entries[index / 16][index % 16];
        let entry_type: RawEntryType = entry[0].into();
        Ok(if !entry_type.is_end_of_directory() { Some(entry) } else { None })
    }
}
