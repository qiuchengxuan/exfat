use alloc::vec::Vec;

use super::allocation_bitmap::AllocationBitmap;
use crate::types::ClusterID;

pub struct Context<IO> {
    pub allocation_bitmap: AllocationBitmap<IO>,
    // Stores first cluster of opened file entry
    pub opened_entries: Vec<ClusterID>,
}

impl<IO> Context<IO> {
    pub fn add_entry(&mut self, cluster_id: ClusterID) -> bool {
        let index = match self.opened_entries.binary_search(&cluster_id) {
            Ok(_) => return false,
            Err(index) => index,
        };
        self.opened_entries.insert(index, cluster_id);
        true
    }

    pub fn remove_entry(&mut self, cluster_id: ClusterID) -> bool {
        let index = match self.opened_entries.binary_search(&cluster_id) {
            Ok(index) => index,
            Err(_) => return false,
        };
        self.opened_entries.remove(index);
        true
    }
}
