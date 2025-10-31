use std::ops::Deref;

use exfat::error::{Error, OperationError};
use exfat::io::Block;
use exfat::{FileOrDirectory, RootDirectory as Root};

use crate::filepath::open;

pub fn truncate<B, E, IO>(root: &mut Root<B, E, IO>, path: &str, size: u64) -> Result<(), Error<E>>
where
    B: Deref<Target = [Block]>,
    E: std::fmt::Debug,
    IO: exfat::io::IO<Block = B, Error = E>,
{
    let mut file = match open(root.open()?, &path)? {
        FileOrDirectory::File(f) => f,
        FileOrDirectory::Directory(_) => return Err(OperationError::NotFile.into()),
    };
    file.truncate(size)
}
