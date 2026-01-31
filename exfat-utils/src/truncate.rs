use std::fmt::Debug;
use std::ops::Deref;

use exfat::FileOrDirectory;
use exfat::error::{Error, OperationError};
use exfat::io::Block;

use crate::filepath::open;
use crate::types::Root;

pub fn truncate<B, E: Debug, IO>(root: &mut Root<IO>, path: &str, size: u64) -> Result<(), Error<E>>
where
    B: Deref<Target = [Block]>,
    IO: for<'a> exfat::io::IO<Block<'a> = B, Error = E>,
{
    let mut file = match open(root.open()?, &path)? {
        FileOrDirectory::File(f) => f,
        FileOrDirectory::Directory(_) => return Err(OperationError::NotFile.into()),
    };
    file.truncate(size)
}
