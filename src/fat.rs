use crate::error::Error;
use crate::io::IOWrapper;
use crate::region::fat::Entry;

#[derive(Clone)]
pub(crate) struct FAT<IO> {
    io: IOWrapper<IO>,
    sector_size: usize,
    offset: u32,
    length: u32,
}

#[deasync::deasync]
impl<E, IO: crate::io::IO<Error = E>> FAT<IO> {
    pub fn new(io: IO, sector_size: usize, offset: u32, length: u32) -> Self {
        Self {
            io: IOWrapper::new(io),
            sector_size,
            offset,
            length,
        }
    }

    pub async fn next_cluster(&mut self, cluster_index: u32) -> Result<Entry, Error<E>> {
        let index = (cluster_index + 2) / (self.sector_size as u32 / 4);
        if index >= self.length {
            return Err(Error::EOF);
        }
        let sector_index = (self.offset + index) as u64;
        let sector = self.io.read(sector_index).await?;
        let offset = (cluster_index as usize + 2) % (self.sector_size / 4);
        let array: &[u32; 128] = unsafe { core::mem::transmute(&sector[offset / 128]) };
        Entry::try_from(u32::from_le(array[offset % 128])).map_err(|_| Error::BadFAT)
    }
}
