#[cfg(all(feature = "async", not(feature = "std")))]
use alloc::boxed::Box;

#[cfg(feature = "async")]
use async_trait::async_trait;

use crate::error::Error;
use crate::types::SectorID;

pub type Block = [u8; 512];

pub(crate) fn flatten(sector: &[Block]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(&sector[0][0], sector.len() * 512) }
}

#[cfg_attr(feature = "async", async_trait)]
#[cfg_attr(not(feature = "async"), deasync::deasync)]
pub trait IO {
    type Error: core::fmt::Debug;
    /// Default to 9, which means 512B
    fn set_sector_size_shift(&mut self, shift: u8) -> Result<(), Self::Error>;
    async fn read<'a>(&'a mut self, id: SectorID) -> Result<&'a [Block], Self::Error>;
    /// Caller guarantees bytes.len() <= SECTOR_SIZE - offset
    async fn write(&mut self, id: SectorID, offset: usize, data: &[u8]) -> Result<(), Self::Error>;
    async fn flush(&mut self) -> Result<(), Self::Error>;
}

pub(crate) struct IOWrapper<IO>(IO);

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<E, T: IO<Error = E>> IOWrapper<T> {
    pub(crate) fn new(io: T) -> Self {
        Self(io)
    }

    pub(crate) async fn read<'a>(&'a mut self, sector: SectorID) -> Result<&'a [Block], Error<E>> {
        self.0.read(sector).await.map_err(|e| Error::IO(e))
    }

    pub(crate) async fn write(
        &mut self,
        id: SectorID,
        offset: usize,
        data: &[u8],
    ) -> Result<(), Error<E>> {
        let result = self.0.write(id, offset, data).await;
        result.map_err(|e| Error::IO(e))
    }

    pub(crate) async fn flush(&mut self) -> Result<(), Error<E>> {
        self.0.flush().await.map_err(|e| Error::IO(e))
    }
}

#[cfg(feature = "std")]
pub mod std;
