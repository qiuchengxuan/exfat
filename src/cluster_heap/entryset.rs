use core::mem::MaybeUninit;

use crate::file::MAX_FILENAME_SIZE;
use crate::fs::{self, SectorRef};
use crate::region::data::entryset::primary::FileDirectory;
use crate::region::data::entryset::secondary::{Secondary, StreamExtension};
use crate::types::SectorID;

#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct EntryID {
    pub sector_id: SectorID,
    pub index: u8, // Max sector size / enty size = 4096 / 32 = 128
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct EntryRef {
    pub sector_ref: SectorRef,
    pub index: u8, // Within sector
}

impl EntryRef {
    pub fn new(sector_ref: SectorRef, index: u8) -> Self {
        Self { sector_ref, index }
    }
}

#[derive(Clone)]
pub struct EntrySet {
    pub(crate) name_bytes: [u8; MAX_FILENAME_SIZE],
    pub(crate) name_length: u8,
    pub file_directory: FileDirectory,
    pub stream_extension: Secondary<StreamExtension>,
    pub(crate) entry_ref: EntryRef,
}

impl Default for EntrySet {
    fn default() -> Self {
        let bytes: MaybeUninit<[u8; MAX_FILENAME_SIZE]> = MaybeUninit::uninit();
        Self {
            name_bytes: unsafe { bytes.assume_init() },
            name_length: 0,
            file_directory: Default::default(),
            stream_extension: Default::default(),
            entry_ref: Default::default(),
        }
    }
}

impl EntrySet {
    pub fn name(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.name_bytes[..self.name_length as usize]) }
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

    pub(crate) fn id(&self, fs_info: &fs::Info) -> EntryID {
        EntryID { sector_id: self.entry_ref.sector_ref.id(fs_info), index: self.entry_ref.index }
    }
}
