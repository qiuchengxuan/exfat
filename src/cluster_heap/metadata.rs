use alloc::string::{String, ToString};

use super::entryset::{EntryIndex, EntrySet};
use crate::region::data::entryset::checksum;
use crate::region::data::entryset::primary::FileDirectory;
use crate::region::data::entryset::secondary::{Secondary, StreamExtension};

#[derive(Clone)]
pub(crate) struct Metadata {
    pub name: String,
    pub file_directory: FileDirectory,
    pub stream_extension: Secondary<StreamExtension>,
    pub entry_index: EntryIndex,
    pub dirty: bool,
}

impl Metadata {
    pub fn new(entryset: EntrySet) -> Self {
        let name = entryset.name().to_string();
        let EntrySet { file_directory, stream_extension, entry_index, .. } = entryset;
        Self { name, file_directory, stream_extension, entry_index, dirty: false }
    }

    pub fn length(&self) -> u64 {
        self.stream_extension.custom_defined.valid_data_length.to_ne()
    }

    pub fn capacity(&self) -> u64 {
        self.stream_extension.data_length.to_ne()
    }

    pub fn set_length(&mut self, length: u64) {
        self.stream_extension.custom_defined.valid_data_length = length.into();
        self.update_checksum();
        self.dirty = true;
    }

    pub fn update_checksum(&mut self) {
        let sum = checksum(&self.file_directory, &self.stream_extension, &self.name);
        self.file_directory.set_checksum = sum.into();
    }
}
