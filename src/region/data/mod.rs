pub(crate) mod entry_type;
pub(crate) mod entryset;

use core::fmt::Debug;

use crate::endian::Little as LE;
use entry_type::RawEntryType;

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub(crate) struct AllocationBitmap {
    pub entry_type: RawEntryType,
    pub bitmap_flags: u8,
    _reserved: [u8; 18],
    pub first_cluster: LE<u32>,
    pub data_length: LE<u64>,
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub(crate) struct UpcaseTable {
    pub entry_type: RawEntryType,
    _reserved1: [u8; 3],
    pub table_checksum: LE<u32>,
    _reserved2: [u8; 12],
    pub first_cluster: LE<u32>,
    pub data_length: LE<u64>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub(crate) struct VolumnLabel {
    pub entry_type: RawEntryType,
    pub character_count: u8,
    pub volumn_label: [LE<u16>; 11],
    _reserved: [u8; 8],
}

impl Into<heapless::String<11>> for VolumnLabel {
    fn into(self) -> heapless::String<11> {
        let mut label: heapless::String<11> = heapless::String::new();
        for i in 0..self.character_count {
            let ch = self.volumn_label[i as usize].to_ne();
            label
                .push(unsafe { char::from_u32_unchecked(ch as u32) })
                .ok();
        }
        label
    }
}

#[derive(Default, Debug)]
pub(crate) struct Checksum(u32);

impl Checksum {
    pub fn write(&mut self, bytes: &[u8]) {
        let mut sum = self.0;
        for &b in bytes.iter() {
            sum = ((sum & 1) << 31) + (sum >> 1) + b as u32;
        }
        self.0 = sum;
    }

    pub fn sum(&self) -> u32 {
        self.0
    }
}
