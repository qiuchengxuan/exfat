pub(crate) mod primary;
pub(crate) mod secondary;

pub(crate) const ENTRY_SIZE: usize = 32;
pub(crate) type RawEntry = [u8; ENTRY_SIZE];

use core::mem::transmute;

use super::entry_type::{EntryType, RawEntryType};
use primary::{Checksum, FileDirectory};
use secondary::{Secondary, StreamExtension};

pub(crate) fn checksum(fd: &FileDirectory, ext: &Secondary<StreamExtension>, name: &str) -> u16 {
    let mut checksum = Checksum::new();
    let array: &[u8; ENTRY_SIZE] = unsafe { transmute(fd) };
    for (i, &value) in array.iter().enumerate() {
        if i == 2 || i == 3 {
            continue;
        }
        checksum.write(value as u16);
    }
    let array: &[u8; ENTRY_SIZE] = unsafe { transmute(ext) };
    for &value in array.iter() {
        checksum.write(value as u16);
    }
    let entry_type = RawEntryType::new(EntryType::Filename, true);
    for (i, ch) in name.chars().enumerate() {
        if i % 15 == 0 {
            checksum.write(u8::from(entry_type) as u16);
            checksum.write(0);
        }
        checksum.write(ch as u8 as u16);
        checksum.write(ch as u16 >> 8);
    }
    for _ in 0..(15 - name.chars().count() % 15) * 2 {
        checksum.write(0);
    }
    checksum.sum()
}
