#[cfg(feature = "std")]
pub mod std;

#[cfg(all(feature = "async", not(feature = "std")))]
use alloc::boxed::Box;
use core::fmt::Debug;
use core::ops::{Deref, DerefMut};

#[cfg(feature = "async")]
use async_trait::async_trait;

use crate::error::Error;
use crate::types::SectorID;

pub const BLOCK_SIZE: usize = 512;
pub type Block = [u8; BLOCK_SIZE];

pub(crate) fn flatten(sector: &[Block]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(&sector[0][0], sector.len() * 512) }
}

#[cfg_attr(feature = "async", async_trait)]
#[cfg_attr(not(feature = "async"), deasync::deasync)]
pub trait IO {
    type Block: Deref<Target = [Block]>;
    type Error: Debug;

    /// Default to 9, which means 512B
    fn set_sector_size_shift(&mut self, shift: u8) -> Result<(), Self::Error>;
    async fn read(&mut self, id: SectorID) -> Result<Self::Block, Self::Error>;
    /// Caller guarantees bytes.len() <= SECTOR_SIZE - offset
    async fn write(&mut self, id: SectorID, offset: usize, data: &[u8]) -> Result<(), Self::Error>;
    async fn flush(&mut self) -> Result<(), Self::Error>;
}

pub(crate) struct Wrapper<D>(D);

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<B: Deref<Target = [Block]>, E, T, D> Wrapper<D>
where
    T: IO<Block = B, Error = E>,
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

impl<'a, B: Deref<Target = [Block]>, E, T, D> Wrap for D
where
    T: IO<Block = B, Error = E>,
    D: DerefMut<Target = T>,
{
    type Output = Wrapper<D>;
    fn wrap(self) -> Self::Output {
        Wrapper(self)
    }
}
