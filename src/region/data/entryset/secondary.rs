use core::{fmt::Debug, mem::MaybeUninit};

use super::super::entry_type::RawEntryType;
use crate::{endian::Little as LE, region::data::entry_type::EntryType};

#[derive(Copy, Clone, Debug)]
pub struct GeneralSecondaryFlags(u8);

impl Default for GeneralSecondaryFlags {
    fn default() -> Self {
        Self(0b1)
    }
}

impl GeneralSecondaryFlags {
    pub fn fat_chain(&self) -> bool {
        (self.0 & 0b10) == 0
    }

    pub fn set_fat_chain(&mut self) {
        self.0 &= !0b10
    }

    pub fn clear_fat_chain(&mut self) {
        self.0 |= 0b10
    }

    pub fn allocation_possible(&self) -> bool {
        self.0 & 1 > 0
    }
}

#[derive(Default)]
#[repr(C, packed(1))]
pub struct Secondary<T: Default> {
    pub(crate) entry_type: RawEntryType,
    pub(crate) general_secondary_flags: GeneralSecondaryFlags,
    pub(crate) custom_defined: T,
    pub(crate) first_cluster: LE<u32>,
    pub(crate) data_length: LE<u64>,
}

impl<T: Default> Secondary<T> {
    pub fn new(custom_defined: T) -> Self {
        Self {
            entry_type: RawEntryType::new(EntryType::StreamExtension, true),
            custom_defined,
            ..Default::default()
        }
    }
}

impl<T: Sized + Default> Clone for Secondary<T> {
    fn clone(&self) -> Self {
        unsafe { core::ptr::read(self) }
    }
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C, packed(1))]
pub struct StreamExtension {
    _reserved1: u8,
    pub name_length: u8,
    pub name_hash: LE<u16>,
    _reserved2: [u8; 2],
    pub valid_data_length: LE<u64>,
    _reserved3: [u8; 4],
}

impl StreamExtension {
    pub fn new(name_length: u8, name_hash: u16) -> Self {
        Self { name_length, name_hash: name_hash.into(), ..Default::default() }
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed(2))]
pub(crate) struct Filename {
    pub entry_type: RawEntryType,
    general_secondary_flags: u8,
    pub filename: MaybeUninit<[u16; 15]>,
}

impl Default for Filename {
    fn default() -> Self {
        Self {
            entry_type: RawEntryType::new(EntryType::Filename, true),
            general_secondary_flags: 0,
            filename: MaybeUninit::uninit(),
        }
    }
}
