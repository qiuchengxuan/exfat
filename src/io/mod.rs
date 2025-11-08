#[cfg(feature = "std")]
pub mod std;

use core::fmt::Debug;
use core::ops::{Deref, DerefMut};

use crate::error::Error;
use crate::types::SectorID;

pub const BLOCK_SIZE: usize = 512;
pub type Block = [u8; BLOCK_SIZE];

pub(crate) fn flatten(sector: &[Block]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(&sector[0][0], sector.len() * 512) }
}

pub trait IO {
    type Block<'a>: Deref<Target = [Block]> + 'a;
    type Error: Debug;

    /// Default to 9, which means 512B
    fn set_sector_size_shift(&mut self, shift: u8) -> Result<(), Self::Error>;

    #[cfg(not(feature = "async"))]
    fn read<'a>(&mut self, id: SectorID) -> Result<Self::Block<'a>, Self::Error>;
    #[cfg(not(feature = "async"))]
    /// Caller guarantees bytes.len() <= SECTOR_SIZE - offset
    fn write(&mut self, id: SectorID, offset: usize, data: &[u8]) -> Result<(), Self::Error>;
    #[cfg(not(feature = "async"))]
    fn flush(&mut self) -> Result<(), Self::Error>;

    #[cfg(feature = "async")]
    fn read<'a>(
        &mut self,
        id: SectorID,
    ) -> impl Future<Output = Result<Self::Block<'a>, Self::Error>>;
    #[cfg(feature = "async")]
    /// Caller guarantees bytes.len() <= SECTOR_SIZE - offset
    fn write(
        &mut self,
        id: SectorID,
        offset: usize,
        data: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>>;
    #[cfg(feature = "async")]
    fn flush(&mut self) -> impl Future<Output = Result<(), Self::Error>>;
}

pub(crate) struct Wrapper<D>(D);

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<B: Deref<Target = [Block]>, E, T, D> Wrapper<D>
where
    T: IO<Block<'static> = B, Error = E>,
    D: DerefMut<Target = T>,
{
    pub fn set_sector_size_shift(&mut self, shift: u8) -> Result<(), Error<E>> {
        self.0.set_sector_size_shift(shift).map_err(|e| Error::IO(e))
    }

    pub async fn read(&mut self, sector: SectorID) -> Result<B, Error<E>> {
        self.0.read(sector).await.map_err(|e| Error::IO(e))
    }

    pub async fn write(&mut self, id: SectorID, idx: usize, data: &[u8]) -> Result<(), Error<E>> {
        let result = self.0.write(id, idx, data).await;
        result.map_err(|e| Error::IO(e))
    }

    pub async fn flush(&mut self) -> Result<(), Error<E>> {
        self.0.flush().await.map_err(|e| Error::IO(e))
    }
}

pub(crate) trait Wrap {
    type Output;
    fn wrap(self) -> Self::Output;
}

impl<B: Deref<Target = [Block]>, E, T, D> Wrap for D
where
    T: IO<Block<'static> = B, Error = E>,
    D: DerefMut<Target = T>,
{
    type Output = Wrapper<D>;
    fn wrap(self) -> Self::Output {
        Wrapper(self)
    }
}
