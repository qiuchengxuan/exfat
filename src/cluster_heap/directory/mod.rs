mod entry_iter;

use core::fmt::Debug;
use core::mem::{self, MaybeUninit};
use core::slice;

use alloc::rc::Rc;

use super::entryset::{EntryIndex, EntrySet};
use super::file::File;
use super::meta::MetaFileDirectory;
use super::metadata::Metadata;
use crate::error::{DataError, Error, ImplementationError, InputError, OperationError};
use crate::file::{FileOptions, MAX_FILENAME_SIZE, TouchOptions};
use crate::fs::SectorIndex;
use crate::region::data::entry_type::{EntryType, RawEntryType};
use crate::region::data::entryset::primary::{DateTime, FileDirectory, name_hash};
use crate::region::data::entryset::secondary::{Filename, Secondary, StreamExtension};
use crate::region::data::entryset::{ENTRY_SIZE, RawEntry, checksum};
use crate::sync::acquire;
use crate::types::ClusterID;
use crate::upcase_table::UpcaseTable;
use entry_iter::EntryIter;

pub struct Directory<E: Debug, IO: crate::io::IO<Error = E>> {
    pub(crate) meta: MetaFileDirectory<IO>,
    pub(crate) upcase_table: Rc<UpcaseTable>,
    #[cfg(feature = "async")]
    closed: bool,
}

impl<E: Debug, IO: crate::io::IO<Error = E>> Directory<E, IO> {
    pub(crate) fn new(meta: MetaFileDirectory<IO>, upcase_table: Rc<UpcaseTable>) -> Self {
        match () {
            #[cfg(not(feature = "async"))]
            _ => Self { meta, upcase_table },
            #[cfg(feature = "async")]
            _ => Self { meta, upcase_table, closed: false },
        }
    }
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
        let mut iter = EntryIter::new(&mut self.meta).await?;
        let mut file_directory: FileDirectory;
        let mut stream_extension: Secondary<StreamExtension>;
        loop {
            let entry = match iter.next().await? {
                Some(entry) => entry,
                None => break,
            };
            let entry_type: RawEntryType = entry[0].into();
            match entry_type.entry_type() {
                Ok(EntryType::FileDirectory) => (),
                Ok(_) => {
                    continue;
                }
                Err(t) => {
                    warn!("Unexpected entry type {}", t);
                    return Err(DataError::Metadata.into());
                }
            };
            file_directory = unsafe { mem::transmute(*entry) };
            if file_directory.secondary_count < 2 {
                return Err(DataError::Metadata.into());
            }
            let entryset_sector_index = iter.sector_index;
            let entryset_index = iter.index;
            let entry = iter.next().await?.unwrap();
            stream_extension = unsafe { mem::transmute(*entry) };
            if !f(&file_directory, &stream_extension) {
                iter.skip(file_directory.secondary_count - 2).await?;
                continue;
            }
            let array: MaybeUninit<[u16; MAX_FILENAME_SIZE / 2]> = MaybeUninit::uninit();
            let mut array: [u16; MAX_FILENAME_SIZE / 2] = unsafe { array.assume_init() };
            for i in 0..(file_directory.secondary_count - 1) as usize {
                if cfg!(feature = "limit-filename-size") && (i + 1) * 15 > array.len() {
                    continue;
                }
                let entry: &Filename = unsafe { mem::transmute(iter.next().await?.unwrap()) };
                let slice = &unsafe { entry.filename.assume_init_ref() }[..];
                array[i * 15..(i + 1) * 15].copy_from_slice(slice);
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
                entry_index: EntryIndex::new(entryset_sector_index, entryset_index as u8),
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
            |entryset| if h(entryset) { Some(entryset.clone()) } else { None },
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
                length as usize == name_length && name_hash == hash
            },
            |entryset| match upcase_table.equals(name, &entryset.name()) {
                true => Some(entryset.clone()),
                false => None,
            },
        )
        .await
    }

    /// Change current directory timestamp
    pub async fn touch(&mut self, datetime: DateTime, opts: TouchOptions) -> Result<(), Error<E>> {
        self.meta.touch(datetime, opts).await?;
        acquire!(self.meta.io).flush().await
    }

    /// Open a file or directory
    pub async fn open(&mut self, entryset: &EntrySet) -> Result<FileOrDirectory<E, IO>, Error<E>> {
        trace!("Open {} on entry-ref {}", entryset.name(), entryset.entry_index);
        let mut context = acquire!(self.meta.context);
        if !context.opened_entries.add(entryset.id(&self.meta.fs_info)) {
            return Err(OperationError::AlreadyOpen.into());
        }
        let cluster_id = entryset.stream_extension.first_cluster.to_ne();
        let file_attributes = entryset.file_directory.file_attributes();
        let sector_index = SectorIndex::new(cluster_id.into(), 0);
        let meta = MetaFileDirectory {
            io: self.meta.io.clone(),
            context: self.meta.context.clone(),
            metadata: Metadata::new(entryset.clone()),
            options: FileOptions::default(),
            sector_index,
            ..self.meta
        };
        let (length, capacity) = (meta.metadata.length(), meta.metadata.capacity());
        trace!("Cluster id {} length {} capacity {}", cluster_id, length, capacity);
        if file_attributes.directory() > 0 {
            let upcase_table = self.upcase_table.clone();
            Ok(FileOrDirectory::Directory(Directory::new(meta, upcase_table)))
        } else {
            Ok(FileOrDirectory::File(File::new(meta, sector_index)))
        }
    }

    async fn lookup_free(&mut self, size: u8) -> Result<(EntryIndex, bool), Error<E>> {
        let mut best: Option<EntryIndex> = None;
        let mut best_count = u8::MAX;

        let mut candidate = EntryIndex::default();
        let mut free_count = 0;

        let mut sector_index = self.meta.sector_index;
        let mut skip = 0;

        loop {
            let mut io = acquire!(self.meta.io);
            let sector = io.read(sector_index.id(&self.meta.fs_info)).await?;
            let entries: &[[RawEntry; 16]] = unsafe { mem::transmute(sector) };
            for (i, entry) in entries.iter().map(|e| e.iter()).flatten().enumerate() {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let entry_type: RawEntryType = entry[0].into();
                if entry_type.is_end_of_directory() {
                    let tail = best.is_none();
                    return Ok((best.unwrap_or(EntryIndex::new(sector_index, i as u8)), tail));
                }
                match (free_count == 0, entry_type.in_use()) {
                    (true, false) => candidate = EntryIndex::new(sector_index, i as u8),
                    (false, false) => free_count += 1,
                    (false, true) => {
                        if free_count >= size && free_count < best_count {
                            best = Some(candidate);
                            best_count = free_count;
                        }
                        free_count = 0;
                    }
                    _ => (),
                }
                if !entry_type.in_use() {
                    continue;
                }
                skip = match entry_type.entry_type() {
                    Ok(EntryType::FileDirectory) => {
                        let file_directory: &FileDirectory = unsafe { mem::transmute(entry) };
                        file_directory.secondary_count
                    }
                    Ok(_) => 0,
                    Err(_) => return Err(DataError::Metadata.into()),
                }
            }
            drop(io);
            sector_index = self.meta.next(sector_index).await?;
        }
    }

    /// Create a file (directory not supported yet)
    pub async fn create(&mut self, name: &str, directory: bool) -> Result<(), Error<E>> {
        if directory {
            return Err(ImplementationError::CreateDirectoryNotSupported.into());
        }
        trace!("Create file {}", name);
        let name_length = name.chars().count();
        if name_length > 255 {
            return Err(InputError::NameTooLong.into());
        }
        if self.find(name).await?.is_some() {
            return Err(OperationError::AlreadyExists.into());
        }

        let num_entries = ((name.len() + 14) / 15) as u8 + 2;
        let (free_entry_index, tail) = self.lookup_free(num_entries).await?;
        let mut write_entry_index = free_entry_index;
        let sector_index = free_entry_index.sector_index;
        let sector_size = self.meta.fs_info.sector_size() as usize;
        let capacity = sector_size / ENTRY_SIZE;
        let out_of_capacity = free_entry_index.index + num_entries + tail as u8 >= capacity as u8;
        if out_of_capacity {
            let sector_index = match self.meta.next(sector_index).await {
                Ok(sector_index) => sector_index,
                Err(Error::Operation(OperationError::EOF)) => {
                    SectorIndex::new(self.meta.allocate(sector_index.cluster_id).await?, 0)
                }
                Err(e) => return Err(e),
            };
            write_entry_index = EntryIndex::new(sector_index, 0);
        }

        debug!("Write entryset at entry-ref {}", write_entry_index);

        let hash = name_hash(&self.upcase_table.to_upper(name));
        let stream_extension = Secondary::new(StreamExtension::new(name.len() as u8, hash));
        let mut file_directory = FileDirectory::new(num_entries - 1, directory);
        let sum = checksum(&file_directory, &stream_extension, name);
        file_directory.set_checksum = sum.into();

        let sector_id = write_entry_index.sector_index.id(&self.meta.fs_info);
        let offset = write_entry_index.index as usize * ENTRY_SIZE;
        let bytes: &[u8; ENTRY_SIZE] = unsafe { mem::transmute(&file_directory) };
        let mut io = acquire!(self.meta.io);
        io.write(sector_id, offset, bytes).await?;
        let bytes: &[u8; ENTRY_SIZE] = unsafe { mem::transmute(&stream_extension) };
        io.write(sector_id, offset + ENTRY_SIZE, bytes).await?;

        let mut chars = name.chars();
        let mut filename = Filename::default();
        for index in 2..(num_entries as usize) {
            let buf = unsafe { filename.filename.assume_init_mut() };
            for i in 0..15 {
                buf[i] = u16::to_le(chars.next().unwrap_or('\0') as u16)
            }
            let bytes: &[u8; ENTRY_SIZE] = unsafe { mem::transmute(&filename) };
            io.write(sector_id, offset + index * ENTRY_SIZE, bytes).await?;
        }
        if tail {
            let offset = offset + (num_entries as usize + 2) * ENTRY_SIZE;
            io.write(sector_id, offset, &[0]).await?;
        };
        // Fill free entries afterwards to avoid corrupting metadata
        if out_of_capacity {
            let sector_id = sector_index.id(&self.meta.fs_info);
            let byte: u8 = RawEntryType::new(EntryType::Filename, false).into();
            for i in free_entry_index.index as usize..(sector_size / ENTRY_SIZE) {
                io.write(sector_id, i * ENTRY_SIZE, &[byte]).await?;
            }
        }
        io.flush().await
    }

    /// Delete a file or directory
    pub async fn delete(&mut self, entryset: &EntrySet) -> Result<(), Error<E>> {
        debug!("Delete file or directory {} entry-ref {}", entryset.name(), entryset.entry_index);
        let file_or_directory = self.open(entryset).await?;
        let meta = match file_or_directory {
            FileOrDirectory::Directory(mut directory) => {
                if directory.walk(|_| true).await?.is_some() {
                    #[cfg(all(feature = "async", not(feature = "std")))]
                    directory.close().await?;
                    return Err(OperationError::DirectoryNotEmpty.into());
                }
                directory.meta.metadata.clone()
            }
            FileOrDirectory::File(file) => file.meta.metadata.clone(),
        };

        let fs_info = self.meta.fs_info;
        let mut sector_id = meta.entry_index.sector_index.id(&fs_info);
        let secondary_count = meta.file_directory.secondary_count as usize;
        let last_index = meta.entry_index.index as usize + secondary_count;
        let sector_size = fs_info.sector_size() as usize;
        let next_sector_id = match last_index * ENTRY_SIZE > sector_size {
            true => self.meta.next(meta.entry_index.sector_index).await?.id(&fs_info),
            false => sector_id,
        };

        let mut offset = meta.entry_index.index as usize * ENTRY_SIZE;
        let mut io = acquire!(self.meta.io);
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
        drop(io);

        let stream_extension = &meta.stream_extension;
        let cluster_id: ClusterID = stream_extension.first_cluster.to_ne().into();
        let fat_chain = meta.stream_extension.general_secondary_flags.fat_chain();
        if cluster_id.valid() {
            let mut context = acquire!(self.meta.context);
            context.allocation_bitmap.release(cluster_id, fat_chain).await?;
        }
        acquire!(self.meta.io).flush().await
    }

    #[cfg(feature = "async")]
    /// `no_std` async only which must be explicitly called
    pub async fn close(mut self) -> Result<(), Error<E>> {
        self.closed = true;
        self.meta.close().await
    }
}

impl<E: core::fmt::Debug, IO: crate::io::IO<Error = E>> Drop for Directory<E, IO> {
    fn drop(&mut self) {
        #[cfg(feature = "async")]
        if !self.closed {
            panic!("Close must be explicitly called");
        }
        #[cfg(not(feature = "async"))]
        self.meta.close().unwrap();
    }
}
