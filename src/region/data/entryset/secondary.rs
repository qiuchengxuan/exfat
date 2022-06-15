use core::fmt::Debug;

use super::super::entry_type::RawEntryType;
use super::generic::Flags;
use crate::endian::Little as LE;

#[derive(Copy, Clone, Debug)]
#[repr(C, packed(1))]
pub struct Secondary<T: Clone + Debug> {
    pub(crate) entry_type: RawEntryType,
    general_secondary_flags: u8,
    pub(crate) custom_defined: T,
    pub(crate) first_cluster: LE<u32>,
    pub(crate) data_length: LE<u64>,
}

impl<T: Clone + Debug> Secondary<T> {
    pub(crate) fn general_secondary_flags(&self) -> Flags {
        Flags(self.general_secondary_flags as u16)
    }
    pub fn data_length(&self) -> u64 {
        self.data_length.to_ne()
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed(1))]
pub struct StreamExtension {
    _reserved1: u8,
    pub name_length: u8,
    pub name_hash: LE<u16>,
    _reserved2: [u8; 2],
    pub valid_data_length: LE<u64>,
    _reserved3: [u8; 4],
}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed(1))]
pub(crate) struct Filename {
    pub entry_type: RawEntryType,
    general_secondary_flags: u8,
    pub filename: [LE<u16>; 15],
}
