use core::fmt::Debug;
use core::ops::Deref;

use crate::cluster_heap::meta::FileSectors;
use crate::error::Error;
use crate::fs::SectorIndex;
use crate::io::{self, Block, Wrap};
use crate::region::data::entry_type::RawEntryType;
use crate::region::data::entryset::{ENTRY_SIZE, RawEntry, entries_from_blocks};
use crate::sync::Share;

pub struct EntryIter<'a, B, IO> {
    sectors: &'a mut FileSectors<IO>,
    blocks: B,
    pub sector_index: SectorIndex,
    pub index: u8,
}

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<'a, B: Deref<Target = [Block]>, E: Debug, IO, S: Share<Target = IO>> EntryIter<'a, B, S>
where
    IO: for<'b> io::IO<Block<'b> = B, Error = E>,
{
    pub async fn new(sectors: &'a mut FileSectors<S>) -> Result<EntryIter<'a, B, S>, Error<E>> {
        let sector_index = sectors.sector_index;
        let mut io = sectors.io.acquire().await.wrap();
        let blocks = io.read(sector_index.sector(&sectors.fs)).await?;
        drop(io);
        Ok(Self { sectors, blocks, sector_index, index: u8::MAX })
    }

    pub async fn skip(&mut self, num_entries: u8) -> Result<(), Error<E>> {
        self.index = self.index.wrapping_add(num_entries);
        let sector_size = self.sectors.fs.sector_size() as usize;
        if (self.index as usize * ENTRY_SIZE) >= sector_size {
            self.index -= (sector_size / ENTRY_SIZE) as u8;
            self.sector_index = self.sectors.next(self.sector_index).await?;
            let mut io = self.sectors.io.acquire().await.wrap();
            self.blocks = io.read(self.sector_index.sector(&self.sectors.fs)).await?;
        }
        Ok(())
    }

    pub async fn next(&mut self) -> Result<Option<RawEntry>, Error<E>> {
        self.skip(1).await?;
        let entries = entries_from_blocks(&self.blocks);
        let entry = entries[self.index as usize];
        let entry_type: RawEntryType = entry[0].into();
        Ok(if !entry_type.is_end_of_directory() { Some(entry) } else { None })
    }
}
