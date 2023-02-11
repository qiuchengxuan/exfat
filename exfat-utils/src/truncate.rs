use exfat::error::{Error, OperationError};
use exfat::{FileOrDirectory, RootDirectory as Root};

use crate::filepath::open;

pub fn truncate<E, IO>(root: &mut Root<E, IO>, path: &str, size: u64) -> Result<(), Error<E>>
where
    E: std::fmt::Debug,
    IO: exfat::io::IO<Error = E>,
{
    let mut file = match open(root.open()?, &path)? {
        FileOrDirectory::File(f) => f,
        FileOrDirectory::Directory(_) => return Err(OperationError::NotFile.into()),
    };
    file.truncate(size)
}
