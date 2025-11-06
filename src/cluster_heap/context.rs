use alloc::vec::Vec;

use super::{allocation_bitmap::AllocationBitmap, entryset::EntryID};

pub struct OpenedEntries {
    pub entries: Vec<EntryID>,
}

impl OpenedEntries {
    pub fn add(&mut self, id: EntryID) -> bool {
        let index = match self.entries.binary_search(&id) {
            Ok(_) => return false,
            Err(index) => index,
        };
        self.entries.insert(index, id);
        true
    }

    pub fn remove(&mut self, id: EntryID) -> bool {
        let index = match self.entries.binary_search(&id) {
            Ok(index) => index,
            Err(_) => return false,
        };
        self.entries.remove(index);
        true
    }
}

pub struct Context<IO> {
    pub allocation_bitmap: AllocationBitmap<IO>,
    // Stores first cluster of opened file entry
    pub opened_entries: OpenedEntries,
}
