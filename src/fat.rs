use crate::error::Error;
use crate::region::fat::Entry;

#[derive(Copy, Clone, Debug)]
pub(crate) struct FAT {
    sector_size: usize,
    offset: u32,
    length: u32,
}

#[deasync::deasync]
impl FAT {
    pub fn new(sector_size: usize, offset: u32, length: u32) -> Self {
        Self {
            sector_size,
            offset,
            length,
        }
    }

    pub async fn next_cluster<E, IO>(
        &self,
        io: &mut IO,
        cluster_index: u32,
    ) -> Result<Entry, Error<E>>
    where
        IO: crate::io::IO<Error = E>,
    {
        let index = (cluster_index + 2) / (self.sector_size as u32 / 4);
        if index >= self.length {
            return Err(Error::EOF);
        }
        let sector_index = (self.offset + index) as u64;
        let sector = io.read(sector_index).await.map_err(|e| Error::IO(e))?;
        let offset = (cluster_index as usize + 2) % (self.sector_size / 4);
        let array: &[u32; 128] = unsafe { core::mem::transmute(&sector[offset / 128]) };
        Entry::try_from(u32::from_le(array[offset % 128])).map_err(|_| Error::BadFAT)
    }
}
