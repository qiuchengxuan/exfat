use super::clusters::SectorRef;
use crate::region::data::entryset::primary::FileDirectory;
use crate::region::data::entryset::secondary::{Secondary, StreamExtension};
use crate::types::SectorID;

#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct EntryID {
    pub sector_id: SectorID,
    pub index: usize,
}

#[derive(Clone, Default)]
pub struct EntrySet {
    pub name: heapless::String<255>,
    pub file_directory: FileDirectory,
    pub stream_extension: Secondary<StreamExtension>,
    pub(crate) sector_ref: SectorRef,
    pub(crate) entry_index: usize,
}

impl EntrySet {
    pub(crate) fn id(&self) -> EntryID {
        EntryID { sector_id: self.sector_ref.id(), index: self.entry_index }
    }

    pub fn in_use(&self) -> bool {
        self.file_directory.entry_type.in_use()
    }

    pub fn data_length(&self) -> u64 {
        self.stream_extension.data_length.to_ne()
    }

    pub fn valid_data_length(&self) -> u64 {
        let valid_data_length = self.stream_extension.custom_defined.valid_data_length;
        valid_data_length.to_ne()
    }
}
