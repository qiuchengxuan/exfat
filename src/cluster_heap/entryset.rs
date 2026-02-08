use core::mem::MaybeUninit;

use derive_more::Display;

use crate::file::MAX_FILENAME_SIZE;
use crate::fs::{self, SectorIndex};
use crate::region::data::entryset::primary::FileDirectory;
use crate::region::data::entryset::secondary::{Secondary, StreamExtension};

#[derive(Copy, Clone, Debug, Display, Eq, Ord, PartialEq, PartialOrd)]
#[display("{}:{}", sector, index)]
pub(crate) struct EntryID {
    pub sector: u64,
    pub index: u8, // Max sector size / enty size = 4096 / 32 = 128
}

#[derive(Copy, Clone, Debug, Default, Display)]
#[display("{}.{}", sector_index, index)]
pub(crate) struct EntryIndex {
    pub sector_index: SectorIndex,
    pub index: u8, // Within sector
}

impl EntryIndex {
    pub fn new(sector_index: SectorIndex, index: u8) -> Self {
        Self { sector_index, index }
    }
}

#[derive(Clone)]
pub struct EntrySet {
    pub(crate) name_bytes: [u8; MAX_FILENAME_SIZE],
    pub(crate) name_length: u8,
    pub file_directory: FileDirectory,
    pub stream_extension: Secondary<StreamExtension>,
    pub(crate) entry_index: EntryIndex,
}

impl Default for EntrySet {
    fn default() -> Self {
        let bytes: MaybeUninit<[u8; MAX_FILENAME_SIZE]> = MaybeUninit::uninit();
        Self {
            name_bytes: unsafe { bytes.assume_init() },
            name_length: 0,
            file_directory: Default::default(),
            stream_extension: Default::default(),
            entry_index: Default::default(),
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

    pub(crate) fn id(&self, fs: &fs::Meta) -> EntryID {
        let sector = self.entry_index.sector_index.sector(fs);
        EntryID { sector, index: self.entry_index.index }
    }
}
