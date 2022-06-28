#[cfg(all(feature = "async", not(feature = "std")))]
use alloc::boxed::Box;

#[cfg(feature = "async")]
use async_trait::async_trait;

use crate::error::Error;

pub type LogicalSector = [u8; 512];

pub(crate) fn flatten(sector: &[LogicalSector]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(&sector[0][0], sector.len() * 512) }
}

#[cfg_attr(feature = "async", async_trait)]
#[deasync::deasync]
pub trait IO: Clone {
    type Error;
    /// Default to 512
    fn set_sector_size(&mut self, size: usize) -> Result<(), Self::Error>;
    async fn read<'a>(&'a mut self, sector: u64) -> Result<&'a [LogicalSector], Self::Error>;
    async fn write(&mut self, sector: u64, offset: usize, buf: &[u8]) -> Result<(), Self::Error>;
}

#[derive(Clone)]
pub(crate) struct IOWrapper<IO>(IO);

#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> IOWrapper<IO> {
    pub(crate) fn new(io: IO) -> Self {
        Self(io)
    }

    pub(crate) async fn read<'a>(&'a mut self, sector: u64) -> Result<&'a [[u8; 512]], Error<E>> {
        self.0.read(sector).await.map_err(|e| Error::IO(e))
    }

    pub(crate) async fn write(
        &mut self,
        sector: u64,
        offset: usize,
        buf: &[u8],
    ) -> Result<(), Error<E>> {
        let result = self.0.write(sector, offset, buf).await;
        result.map_err(|e| Error::IO(e))
    }
}

#[cfg(feature = "std")]
pub mod std {
    #[cfg(feature = "async")]
    use async_std as std_;
    #[cfg(not(feature = "async"))]
    use std as std_;

    use std_::{
        fs::File,
        io::prelude::*,
        io::SeekFrom,
        mem::MaybeUninit,
        path::Path,
        slice::from_raw_parts,
        sync::{Arc, Mutex},
    };

    #[cfg(feature = "async")]
    use async_trait::async_trait;

    use crate::region::boot::MAX_SECTOR_SIZE;

    #[derive(Debug)]
    pub struct FileIO {
        file: Arc<Mutex<File>>,
        sector_size: usize,
        buffer: MaybeUninit<[u8; MAX_SECTOR_SIZE]>,
    }

    impl Clone for FileIO {
        fn clone(&self) -> Self {
            Self {
                file: self.file.clone(),
                sector_size: self.sector_size,
                buffer: MaybeUninit::uninit(),
            }
        }
    }

    #[deasync::deasync]
    impl FileIO {
        pub async fn open<P: AsRef<Path>>(filepath: P) -> std::io::Result<Self> {
            let mut options = match () {
                #[cfg(feature = "async")]
                () => async_std::fs::OpenOptions::new(),
                #[cfg(not(feature = "async"))]
                () => File::options(),
            };
            let result = options.read(true).write(true).open(filepath).await;
            result.map(|file| Self {
                file: Arc::new(Mutex::new(file)),
                sector_size: 512,
                buffer: MaybeUninit::uninit(),
            })
        }
    }

    #[cfg_attr(feature = "async", async_trait)]
    #[deasync::deasync]
    impl super::IO for FileIO {
        type Error = std::io::Error;

        fn set_sector_size(&mut self, size: usize) -> Result<(), Self::Error> {
            self.sector_size = size;
            Ok(())
        }

        async fn read<'a>(&'a mut self, sector: u64) -> Result<&'a [[u8; 512]], Self::Error> {
            let seek = SeekFrom::Start(sector * self.sector_size as u64);

            let mut file = match () {
                #[cfg(not(feature = "async"))]
                () => self.file.lock().unwrap(),
                #[cfg(feature = "async")]
                () => self.file.lock().await,
            };
            file.seek(seek).await?;
            let buffer = unsafe { self.buffer.assume_init_mut() };
            file.read_exact(&mut buffer[..self.sector_size]).await?;

            Ok(unsafe { from_raw_parts(buffer.as_ptr() as *const _, self.sector_size / 512) })
        }

        async fn write(
            &mut self,
            sector: u64,
            offset: usize,
            buf: &[u8],
        ) -> Result<(), Self::Error> {
            let mut file = match () {
                #[cfg(not(feature = "async"))]
                () => self.file.lock().unwrap(),
                #[cfg(feature = "async")]
                () => self.file.lock().await,
            };
            let seek = SeekFrom::Start(sector * self.sector_size as u64 + offset as u64);
            file.seek(seek).await?;
            file.write_all(buf).await.map(|_| ())
        }
    }
}
