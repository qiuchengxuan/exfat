#[cfg(feature = "std")]
pub mod std;

use core::ops::{Deref, DerefMut};

use crate::error::Error;

pub const BLOCK_SIZE: usize = 512;
pub type Block = [u8; BLOCK_SIZE];

pub(crate) fn flatten(blocks: &[Block]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(&blocks[0][0], blocks.len() * BLOCK_SIZE) }
}

pub trait ErrorType {
    type Error;
}

#[cfg(not(feature = "async"))]
pub trait Write: ErrorType {
    /// Caller guarantees bytes.len() <= SECTOR_SIZE - offset
    fn write(&mut self, sector: u64, offset: usize, data: &[u8]) -> Result<(), Self::Error>;

    fn flush(&mut self) -> Result<(), Self::Error>;
}

#[cfg(feature = "async")]
pub trait Write: ErrorType {
    /// Caller guarantees bytes.len() <= SECTOR_SIZE - offset
    fn write(
        &mut self, sector: u64, offset: usize, data: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>>;

    fn flush(&mut self) -> impl Future<Output = Result<(), Self::Error>>;
}

pub trait Read: ErrorType {
    type Block<'a>: Deref<Target = [Block]> + 'a;

    #[cfg(not(feature = "async"))]
    fn read<'a>(&mut self, sector: u64) -> Result<Self::Block<'a>, Self::Error>;

    #[cfg(feature = "async")]
    fn read<'a>(
        &mut self, sector: u64,
    ) -> impl Future<Output = Result<Self::Block<'a>, Self::Error>>;
}

pub trait IO: Read + Write {
    /// Default to 9, which means 512B
    fn set_sector_size_shift(&mut self, shift: u8) -> Result<(), Self::Error>;
}

pub(crate) struct Wrapper<D>(D);

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
impl<E, T, D> Wrapper<D>
where
    T: IO<Error = E>,
    D: DerefMut<Target = T>,
{
    pub fn set_sector_size_shift(&mut self, shift: u8) -> Result<(), Error<E>> {
        self.0.set_sector_size_shift(shift).map_err(|e| Error::IO(e))
    }

    pub async fn read<'a>(&'a mut self, sector: u64) -> Result<<T as Read>::Block<'a>, Error<E>> {
        self.0.read(sector).await.map_err(|e| Error::IO(e))
    }

    pub async fn write(&mut self, sector: u64, offset: usize, data: &[u8]) -> Result<(), Error<E>> {
        self.0.write(sector, offset, data).await.map_err(|e| Error::IO(e))
    }

    pub async fn flush(&mut self) -> Result<(), Error<E>> {
        self.0.flush().await.map_err(|e| Error::IO(e))
    }
}

pub(crate) trait Wrap {
    type Output;
    fn wrap(self) -> Self::Output;
}

impl<E, T, D> Wrap for D
where
    T: IO<Error = E>,
    D: DerefMut<Target = T>,
{
    type Output = Wrapper<D>;
    fn wrap(self) -> Self::Output {
        Wrapper(self)
    }
}
