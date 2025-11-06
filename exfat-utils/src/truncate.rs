use std::fmt::Debug;

use exfat::FileOrDirectory;
use exfat::error::{Error, OperationError};

use crate::filepath::open;
use crate::types::Root;

pub fn truncate<E: Debug, IO>(root: &mut Root<IO>, path: &str, size: u64) -> Result<(), Error<E>>
where
    IO: exfat::io::IO<Error = E>,
{
    let mut file = match open(root.open()?, &path)? {
        FileOrDirectory::File(f) => f,
        FileOrDirectory::Directory(_) => return Err(OperationError::NotFile.into()),
    };
    file.truncate(size)
}
