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
        path::Path,
        sync::{Arc, Mutex},
    };

    #[cfg(feature = "async")]
    use async_trait::async_trait;

    #[derive(Debug)]
    pub struct FileIO {
        file: Arc<Mutex<File>>,
        sector_size: usize,
        sector: Option<u64>,
        buffer: Vec<[u8; 512]>,
    }

    impl Clone for FileIO {
        fn clone(&self) -> Self {
            Self {
                file: self.file.clone(),
                sector_size: self.sector_size,
                sector: None,
                buffer: vec![[0u8; 512]; self.sector_size / 512],
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
                sector: None,
                buffer: vec![[0u8; 512]; 1],
            })
        }
    }

    #[cfg_attr(feature = "async", async_trait)]
    #[deasync::deasync]
    impl super::IO for FileIO {
        type Error = std::io::Error;

        fn set_sector_size(&mut self, size: usize) -> Result<(), Self::Error> {
            self.sector_size = size;
            self.sector = None;
            self.buffer = vec![[0u8; 512]; size / 512];
            Ok(())
        }

        async fn read<'a>(&'a mut self, sector: u64) -> Result<&'a [[u8; 512]], Self::Error> {
            if self.sector == Some(sector) {
                return Ok(self.buffer.as_ref());
            }
            let seek = SeekFrom::Start(sector * self.sector_size as u64);

            let mut file = match () {
                #[cfg(not(feature = "async"))]
                () => self.file.lock().unwrap(),
                #[cfg(feature = "async")]
                () => self.file.lock().await,
            };
            file.seek(seek).await?;
            self.sector = Some(sector);
            for buf in self.buffer.iter_mut() {
                match file.read_exact(buf).await {
                    Ok(_) => continue,
                    Err(e) => return Err(e),
                }
            }
            Ok(self.buffer.as_ref())
        }

        async fn write(
            &mut self,
            sector: u64,
            offset: usize,
            buf: &[u8],
        ) -> Result<(), Self::Error> {
            if self.sector == Some(sector) {
                self.sector = None;
            }
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
