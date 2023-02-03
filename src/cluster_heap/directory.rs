use core::fmt::Debug;
use core::mem::{self, MaybeUninit};
use core::slice;

use alloc::rc::Rc;

use super::entry::{name_hash, with_context};
use super::entry::{ClusterEntry, Meta};
use super::entryset::{EntryRef, EntrySet};
use super::file::File;
use crate::error::{DataError, Error, OperationError};
use crate::file::{FileOptions, TouchOptions, MAX_FILENAME_SIZE};
use crate::fs::SectorRef;
use crate::region::data::entry_type::{EntryType, RawEntryType};
use crate::region::data::entryset::primary::{DateTime, FileDirectory};
use crate::region::data::entryset::secondary::{Filename, Secondary, StreamExtension};
use crate::region::data::entryset::{RawEntry, ENTRY_SIZE};
use crate::types::ClusterID;
use crate::upcase_table::UpcaseTable;

pub struct Directory<E: Debug, IO: crate::io::IO<Error = E>> {
    pub(crate) entry: ClusterEntry<IO>,
    pub(crate) upcase_table: Rc<UpcaseTable>,
}

pub enum FileOrDirectory<E: Debug, IO: crate::io::IO<Error = E>> {
    File(File<E, IO>),
    Directory(Directory<E, IO>),
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E: Debug, IO: crate::io::IO<Error = E>> Directory<E, IO> {
    async fn walk_matches<F, H, R>(&mut self, f: F, mut h: H) -> Result<Option<R>, Error<E>>
    where
        F: Fn(&FileDirectory, &Secondary<StreamExtension>) -> bool,
        H: FnMut(&EntrySet) -> Option<R>,
    {
        let fs_info = self.entry.fs_info;
        let sector_size = fs_info.sector_size() as usize;
        let mut sector_ref = self.entry.sector_ref;
        let mut index = 0;
        let sector = self.entry.io.read(sector_ref.id(&fs_info)).await?;
        let mut entries: &[[RawEntry; 16]] = unsafe { mem::transmute(sector) };
        let mut file_directory: FileDirectory;
        let mut stream_extension: Secondary<StreamExtension>;
        loop {
            let entry = &entries[index / 16][index % 16];
            let entry_type: RawEntryType = entry[0].into();
            if entry_type.is_end_of_directory() {
                break;
            }
            match entry_type.entry_type() {
                Ok(EntryType::FileDirectory) => (),
                Ok(_) => {
                    index += 1;
                    continue;
                }
                Err(t) => {
                    warn!("Unexpected entry type {}", t);
                    return Err(DataError::Metadata.into());
                }
            };
            file_directory = unsafe { mem::transmute(*entry) };
            let entryset_sector_ref = sector_ref;
            let entryset_index = index;
            let mut secondary_remain = file_directory.secondary_count - 1;
            index += 1;
            if (index * ENTRY_SIZE) >= sector_size {
                index -= sector_size / ENTRY_SIZE;
                sector_ref = self.entry.next(sector_ref).await?;
                let sector = self.entry.io.read(sector_ref.id(&fs_info)).await?;
                entries = unsafe { mem::transmute(sector) }
            }
            let entry = &entries[index / 16][index % 16];
            stream_extension = unsafe { mem::transmute(*entry) };
            if !f(&file_directory, &stream_extension) {
                index += secondary_remain as usize;
                continue;
            }
            let array: MaybeUninit<[u16; MAX_FILENAME_SIZE / 2]> = MaybeUninit::uninit();
            let mut array: [u16; MAX_FILENAME_SIZE / 2] = unsafe { array.assume_init() };
            let mut cursor = 0;
            while secondary_remain > 0 {
                secondary_remain -= 1;
                index += 1;
                if (index * ENTRY_SIZE) >= sector_size {
                    index -= sector_size / ENTRY_SIZE;
                    sector_ref = self.entry.next(sector_ref).await?;
                    let sector = self.entry.io.read(sector_ref.id(&fs_info)).await?;
                    entries = unsafe { mem::transmute(sector) }
                }
                if cfg!(feature = "limit-max-filename-size") && cursor + 15 > array.len() {
                    continue;
                }
                let entry: &Filename = unsafe { mem::transmute(&entries[index / 16][index % 16]) };
                array[cursor..cursor + 15].copy_from_slice(&entry.filename[..]);
                cursor += 15;
            }
            let name_length = stream_extension.custom_defined.name_length as usize;
            for i in 0..name_length {
                array[i] = u16::from_le(array[i]);
            }
            let slice = unsafe { slice::from_raw_parts(&array[0], name_length) };
            let mut buf: [u8; MAX_FILENAME_SIZE] = unsafe { mem::transmute(array) };
            let mut cursor = 0;
            for &ch in slice {
                let ch = unsafe { char::from_u32_unchecked(ch as u32) };
                ch.encode_utf8(&mut buf[cursor..]);
                cursor += ch.len_utf8();
            }
            let entryset = EntrySet {
                name_bytes: buf,
                name_length: cursor as u8,
                file_directory,
                stream_extension,
                entry_ref: EntryRef::new(entryset_sector_ref, entryset_index as u8),
            };
            if let Some(retval) = h(&entryset) {
                return Ok(Some(retval));
            }
        }
        Ok(None)
    }

    /// Walk through directory, including not inuse entries
    pub async fn walk<H>(&mut self, mut h: H) -> Result<Option<EntrySet>, Error<E>>
    where
        H: FnMut(&EntrySet) -> bool,
    {
        self.walk_matches(
            |_, _| true,
            |entryset| {
                if h(entryset) {
                    Some(entryset.clone())
                } else {
                    None
                }
            },
        )
        .await
    }

    /// Find a file or directory matching specified name
    pub async fn find(&mut self, name: &str) -> Result<Option<EntrySet>, Error<E>> {
        let name_length = name.chars().count();
        let upcase_table = self.upcase_table.clone();
        let hash = name_hash(&self.upcase_table.to_upper(name));
        self.walk_matches(
            |file_directory, stream_extension| -> bool {
                let entry_type = file_directory.entry_type;
                if !entry_type.in_use() {
                    return false;
                }
                let length = stream_extension.custom_defined.name_length;
                let name_hash = stream_extension.custom_defined.name_hash.to_ne();
                if length as usize != name_length || name_hash != hash {
                    return false;
                }
                true
            },
            |entryset| {
                if upcase_table.equals(name, &entryset.name()) {
                    Some(entryset.clone())
                } else {
                    None
                }
            },
        )
        .await
    }

    /// Change current directory timestamp
    pub async fn touch(&mut self, datetime: DateTime, opts: TouchOptions) -> Result<(), Error<E>> {
        self.entry.touch(datetime, opts).await?;
        self.entry.io.flush().await
    }

    /// Open a file or directory
    pub async fn open(&mut self, entryset: &EntrySet) -> Result<FileOrDirectory<E, IO>, Error<E>> {
        trace!("Open {}", entryset.name());
        let mut context = with_context!(self.entry.context);
        if !context.opened_entries.add(entryset.id(&self.entry.fs_info)) {
            return Err(OperationError::AlreadyOpen.into());
        }
        let cluster_id = entryset.stream_extension.first_cluster.to_ne();
        let file_attributes = entryset.file_directory.file_attributes();
        let sector_ref = SectorRef::new(cluster_id.into(), 0);
        let entry = ClusterEntry {
            io: self.entry.io.clone(),
            context: self.entry.context.clone(),
            meta: Meta::new(entryset.clone()),
            options: FileOptions::default(),
            sector_ref,
            ..self.entry
        };
        let (length, capacity) = (entry.meta.length(), entry.meta.capacity());
        trace!("Cluster id {} length {} capacity {}", cluster_id, length, capacity);
        if file_attributes.directory() > 0 {
            let upcase_table = self.upcase_table.clone();
            Ok(FileOrDirectory::Directory(Directory { entry, upcase_table }))
        } else {
            Ok(FileOrDirectory::File(File::new(entry, sector_ref)))
        }
    }

    /// Delete a file or directory
    pub async fn delete(&mut self, entryset: &EntrySet) -> Result<(), Error<E>> {
        debug!("Delete file or directory {}", entryset.name());
        let file_or_directory = self.open(entryset).await?;
        let meta = match file_or_directory {
            FileOrDirectory::Directory(mut directory) => {
                if directory.walk(|_| true).await?.is_some() {
                    #[cfg(all(feature = "async", not(feature = "std")))]
                    directory.close().await?;
                    return Err(OperationError::DirectoryNotEmpty.into());
                }
                directory.entry.meta.clone()
            }
            FileOrDirectory::File(file) => file.entry.meta.clone(),
        };

        let fs_info = self.entry.fs_info;
        let mut sector_id = meta.entry_ref.sector_ref.id(&fs_info);
        let secondary_count = meta.file_directory.secondary_count as usize;
        let last_index = meta.entry_ref.index as usize + secondary_count;
        let sector_size = fs_info.sector_size() as usize;
        let next_sector_id = match last_index * ENTRY_SIZE > sector_size {
            true => self.entry.next(meta.entry_ref.sector_ref).await?.id(&fs_info),
            false => sector_id,
        };

        let mut offset = meta.entry_ref.index as usize * ENTRY_SIZE;
        let io = &mut self.entry.io;
        io.write(sector_id, offset, &[EntryType::FileDirectory.into(); 1]).await?;
        offset = (offset + ENTRY_SIZE) % sector_size;
        if offset == 0 {
            sector_id = next_sector_id;
        }
        io.write(sector_id, offset, &[EntryType::StreamExtension.into(); 1]).await?;
        for _ in 0..(secondary_count - 1) {
            offset = (offset + ENTRY_SIZE) % sector_size;
            if offset == 0 {
                sector_id = next_sector_id;
            }
            io.write(sector_id, offset, &[EntryType::Filename.into(); 1]).await?;
        }

        let stream_extension = &meta.stream_extension;
        let cluster_id: ClusterID = stream_extension.first_cluster.to_ne().into();
        let fat_chain = meta.stream_extension.general_secondary_flags.fat_chain();
        if cluster_id.valid() {
            let mut context = with_context!(self.entry.context);
            context.allocation_bitmap.release(cluster_id, fat_chain).await?;
        }
        io.flush().await
    }

    #[cfg(all(feature = "async", not(feature = "std")))]
    /// `no_std` async only which must be explicitly called
    pub async fn close(mut self) -> Result<(), Error<E>> {
        self.entry.close().await
    }
}

#[cfg(any(not(feature = "async"), feature = "std"))]
impl<E: core::fmt::Debug, IO: crate::io::IO<Error = E>> Drop for Directory<E, IO> {
    fn drop(&mut self) {
        match () {
            #[cfg(all(feature = "async", not(feature = "std")))]
            () => panic!("Close must be explicit called"),
            #[cfg(all(feature = "async", feature = "std"))]
            () => async_std::task::block_on(self.entry.close()).unwrap(),
            #[cfg(not(feature = "async"))]
            () => self.entry.close().unwrap(),
        }
    }
}
