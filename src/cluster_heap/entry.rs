use alloc::sync::Arc;
use core::mem::transmute;

use super::clusters::SectorRef;
use super::context::Context;
use super::entryset::{EntryID, EntrySet};
use crate::error::Error;
use crate::fat;
use crate::io::IOWrapper;
use crate::region::data::entry_type::{EntryType, RawEntryType};
use crate::region::data::entryset::primary::{DateTime, FileDirectory};
use crate::region::data::entryset::secondary::{Secondary, StreamExtension};
use crate::region::data::entryset::{RawEntry, ENTRY_SIZE};
use crate::region::fat::Entry;
use crate::sync::Mutex;
use crate::types::ClusterID;

#[macro_export]
macro_rules! with_context {
    ($context: expr) => {
        match () {
            #[cfg(feature = "async")]
            _ => $context.lock().await,
            #[cfg(not(feature = "async"))]
            _ => $context.lock().unwrap(),
        }
    };
}

pub(crate) use with_context;

pub(crate) struct Checksum(u16);

impl Checksum {
    pub(crate) fn new() -> Self {
        Self(0)
    }

    pub(crate) fn write(&mut self, value: u16) {
        self.0 = if self.0 & 1 > 0 { 0x8000 } else { 0 } + (self.0 >> 1) + value
    }

    pub(crate) fn sum(&self) -> u16 {
        self.0
    }
}

pub(crate) fn name_hash(name: &str) -> u16 {
    let mut checksum = Checksum::new();
    for ch in name.chars() {
        checksum.write(ch as u16);
        checksum.write(((ch as u32) >> 16) as u16);
    }
    checksum.sum()
}

#[derive(Clone)]
pub(crate) struct Meta {
    pub name: heapless::String<255>,
    pub file_directory: FileDirectory,
    pub stream_extension: Secondary<StreamExtension>,
    pub sector_ref: SectorRef,
    pub entry_index: u8, // Within sector
    pub dirty: bool,
}

impl Meta {
    pub fn new(entryset: EntrySet) -> Self {
        let EntrySet { name, file_directory, stream_extension, sector_ref, entry_index } = entryset;
        Self { name, file_directory, stream_extension, sector_ref, entry_index, dirty: false }
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
        let mut checksum = Checksum::new();
        let array: &[u8; ENTRY_SIZE] = unsafe { transmute(&self.file_directory) };
        for (i, &value) in array.iter().enumerate() {
            if i == 2 || i == 3 {
                continue;
            }
            checksum.write(value as u16);
        }
        let array: &[u8; ENTRY_SIZE] = unsafe { transmute(&self.stream_extension) };
        for &value in array.iter() {
            checksum.write(value as u16);
        }
        let entry_type = RawEntryType::new(EntryType::Filename, true);
        for (i, ch) in self.name.chars().enumerate() {
            if i % 15 == 0 {
                checksum.write(u8::from(entry_type) as u16);
                checksum.write(0);
            }
            checksum.write(ch as u8 as u16);
            checksum.write(ch as u16 >> 8);
        }
        for _ in 0..(15 - self.name.len() % 15) * 2 {
            checksum.write(0);
        }
        self.file_directory.set_checksum = checksum.sum().into();
    }
}

#[derive(Clone)]
pub(crate) struct ClusterEntry<IO> {
    pub io: IOWrapper<IO>,
    pub context: Arc<Mutex<Context<IO>>>,
    pub fat_info: fat::Info,
    pub meta: Meta,
    pub sector_ref: SectorRef,
    pub sector_size_shift: u8,
}

impl<IO> ClusterEntry<IO> {
    pub(crate) fn id(&self) -> EntryID {
        EntryID { sector_id: self.meta.sector_ref.id(), index: self.meta.entry_index }
    }
}

#[derive(Copy, Clone)]
pub struct TouchOption {
    pub access: bool,
    pub modified: bool,
}

impl Default for TouchOption {
    fn default() -> Self {
        Self { access: true, modified: true }
    }
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E, IO: crate::io::IO<Error = E>> ClusterEntry<IO> {
    pub fn cluster_size_shift(&self) -> u8 {
        self.sector_size_shift + self.sector_ref.sectors_per_cluster_shift
    }

    pub async fn next(&mut self, sector_ref: SectorRef) -> Result<SectorRef, Error<E>> {
        let not_fat_chain = self.meta.stream_extension.general_secondary_flags.not_fat_chain();
        if !sector_ref.is_last_sector_in_cluster() || not_fat_chain {
            return Ok(sector_ref.next());
        }
        let cluster_id = sector_ref.cluster_id;
        let sector_id = self.fat_info.fat_sector_id(cluster_id).ok_or(Error::EOF)?;
        let sector = self.io.read(sector_id).await?;
        match self.fat_info.next_cluster_id(sector, sector_ref.cluster_id) {
            Ok(Entry::Next(cluster_id)) => Ok(sector_ref.new(cluster_id, 0)),
            _ => Err(Error::EOF),
        }
    }

    pub async fn touch(&mut self, datetime: DateTime, option: TouchOption) -> Result<(), Error<E>> {
        let meta = &mut self.meta;
        if option.access {
            meta.file_directory.update_last_accessed_timestamp(datetime);
        }
        if option.modified {
            meta.file_directory.update_last_modified_timestamp(datetime);
        }
        meta.update_checksum();
        Ok(())
    }
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E, IO: crate::io::IO<Error = E>> ClusterEntry<IO> {
    pub async fn allocate(&mut self, last: ClusterID) -> Result<ClusterID, Error<E>> {
        let mut context = with_context!(self.context);
        // XXX: prefer next cluster-id
        let cluster_id = context.allocation_bitmap.allocate().await?;

        let cluster_size = 1 << self.cluster_size_shift();
        let meta = &mut self.meta;

        if !last.valid() {
            meta.stream_extension.first_cluster = u32::from(cluster_id).into();
        } else if last + 1u32 != cluster_id {
            if meta.stream_extension.general_secondary_flags.not_fat_chain() {
                let first_cluster = self.sector_ref.cluster_id;
                for i in 0..(meta.capacity() / cluster_size - 1) {
                    let cluster_id = first_cluster + i as u32;
                    let next = cluster_id + 1u32;
                    let sector_id = self.fat_info.fat_sector_id(cluster_id).unwrap();
                    let bytes = u32::to_le_bytes(next.into());
                    self.io.write(sector_id, self.fat_info.offset(next), &bytes).await?;
                }
                meta.stream_extension.general_secondary_flags.set_fat_chain();
            }
            let sector_id = self.fat_info.fat_sector_id(last).unwrap();
            let bytes = u32::to_le_bytes(cluster_id.into());
            self.io.write(sector_id, self.fat_info.offset(last), &bytes).await?;
        }
        meta.stream_extension.data_length = (meta.capacity() + cluster_size).into();
        meta.update_checksum();
        Ok(cluster_id)
    }

    pub async fn sync(&mut self) -> Result<(), Error<E>> {
        let meta = &mut self.meta;
        if !meta.sector_ref.cluster_id.valid() {
            // Probably root directory
            return Ok(());
        }
        if meta.dirty {
            trace!("Flush metadata since dirty");
            let mut sector_id = meta.sector_ref.id();
            let bytes: &RawEntry = unsafe { transmute(&meta.file_directory) };
            let offset = meta.entry_index as usize * ENTRY_SIZE;
            self.io.write(sector_id, offset, &bytes[..]).await?;
            let mut offset = (meta.entry_index as usize + 1) * ENTRY_SIZE;
            if offset == 1 << self.sector_size_shift {
                offset = 0;
                sector_id += 1u32;
            }
            let bytes: &RawEntry = unsafe { transmute(&meta.stream_extension) };
            self.io.write(sector_id, offset, &bytes[..]).await?;
            self.io.flush().await?;
            meta.dirty = false;
        }
        Ok(())
    }

    pub async fn close(&mut self) -> Result<(), Error<E>> {
        self.sync().await?;
        let mut context = with_context!(self.context);
        context.opened_entries.remove(self.id());
        Ok(())
    }
}
