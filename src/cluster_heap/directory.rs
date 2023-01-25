use core::fmt::Debug;
use core::mem;

use alloc::rc::Rc;

use super::entry::{name_hash, with_context};
use super::entry::{ClusterEntry, Meta, TouchOption};
use super::entryset::EntrySet;
use super::file::File;
use crate::error::Error;
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
        let sector_size = 1 << self.entry.sector_size_shift;
        let mut sector_ref = self.entry.sector_ref;
        let mut index = 0;
        let sector = self.entry.io.read(sector_ref.id()).await?;
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
                    return Err(Error::Generic("Unexpected entry type"));
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
                let sector = self.entry.io.read(sector_ref.id()).await?;
                entries = unsafe { mem::transmute(sector) }
            }
            let entry = &entries[index / 16][index % 16];
            stream_extension = unsafe { mem::transmute(*entry) };
            if !f(&file_directory, &stream_extension) {
                index += secondary_remain as usize;
                continue;
            }
            let mut entry_name = heapless::String::<255>::new();
            while secondary_remain > 0 {
                secondary_remain -= 1;
                index += 1;
                if (index * ENTRY_SIZE) >= sector_size {
                    index -= sector_size / ENTRY_SIZE;
                    sector_ref = self.entry.next(sector_ref).await?;
                    let sector = self.entry.io.read(sector_ref.id()).await?;
                    entries = unsafe { mem::transmute(sector) }
                }
                let entry: &Filename = unsafe { mem::transmute(&entries[index / 16][index % 16]) };
                for ch in entry.filename {
                    let ch = unsafe { char::from_u32_unchecked(ch.to_ne() as u32) };
                    entry_name.push(ch).ok();
                }
            }
            entry_name.truncate(stream_extension.custom_defined.name_length as usize);
            let entry_set = EntrySet {
                name: entry_name,
                file_directory,
                stream_extension,
                sector_ref: entryset_sector_ref,
                entry_index: entryset_index as u8,
            };
            if let Some(retval) = h(&entry_set) {
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
        let upcase_table = self.upcase_table.clone();
        let hash = name_hash(&self.upcase_table.to_upper(name));
        self.walk_matches(
            |file_directory, stream_extension| -> bool {
                let entry_type = file_directory.entry_type;
                if !entry_type.in_use() {
                    return false;
                }
                let name_length = stream_extension.custom_defined.name_length;
                let name_hash = stream_extension.custom_defined.name_hash.to_ne();
                if name_length as usize != name.len() || name_hash != hash {
                    return false;
                }
                true
            },
            |entryset| {
                if upcase_table.equals(name, &entryset.name) {
                    Some(entryset.clone())
                } else {
                    None
                }
            },
        )
        .await
    }

    /// Change current directory timestamp
    pub async fn touch(&mut self, datetime: DateTime, option: TouchOption) -> Result<(), Error<E>> {
        self.entry.touch(datetime, option).await?;
        self.entry.io.flush().await
    }

    /// Open a file or directory
    pub async fn open(&mut self, name: &str) -> Result<FileOrDirectory<E, IO>, Error<E>> {
        debug!("Open file {}", name);
        let future = self.find(name);
        let entryset = future.await?.ok_or(Error::NoSuchFileOrDirectory)?;
        let mut context = with_context!(self.entry.context);
        if !context.opened_entries.add(entryset.id()) {
            return Err(Error::AlreadyOpen);
        }
        let cluster_id = entryset.stream_extension.first_cluster.to_ne();
        let file_attributes = entryset.file_directory.file_attributes();
        let sector_ref = self.entry.sector_ref.new(cluster_id.into(), 0);
        let entry = ClusterEntry {
            io: self.entry.io.clone(),
            context: self.entry.context.clone(),
            fat_info: self.entry.fat_info,
            meta: Meta::new(entryset),
            sector_ref,
            sector_size_shift: self.entry.sector_size_shift,
        };
        trace!("Cluster id {}", cluster_id);
        if file_attributes.directory() > 0 {
            let upcase_table = self.upcase_table.clone();
            Ok(FileOrDirectory::Directory(Directory { entry, upcase_table }))
        } else {
            Ok(FileOrDirectory::File(File::new(entry, sector_ref)))
        }
    }

    /// Delete a file or directory
    pub async fn delete(&mut self, name: &str) -> Result<(), Error<E>> {
        debug!("Delete file or directory {}", name);
        let file_or_directory = self.open(name).await?;
        let meta = match file_or_directory {
            FileOrDirectory::Directory(mut directory) => {
                if directory.walk(|_| true).await?.is_some() {
                    #[cfg(all(feature = "async", not(feature = "std")))]
                    directory.close().await?;
                    return Err(Error::DirectoryNotEmpty);
                }
                directory.entry.meta.clone()
            }
            FileOrDirectory::File(file) => file.entry.meta.clone(),
        };

        let mut sector_id = meta.sector_ref.id();
        let secondary_count = meta.file_directory.secondary_count as usize;
        let last_index = meta.entry_index as usize + secondary_count;
        let sector_size = 1 << self.entry.sector_size_shift;
        let next_sector_id = match last_index * ENTRY_SIZE > sector_size {
            true => self.entry.next(meta.sector_ref).await?.id(),
            false => sector_id,
        };

        let mut offset = meta.entry_index as usize * ENTRY_SIZE;
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
        let chain = !meta.stream_extension.general_secondary_flags.not_fat_chain();
        if cluster_id.valid() {
            let mut context = with_context!(self.entry.context);
            context.allocation_bitmap.release(cluster_id, chain).await?;
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
