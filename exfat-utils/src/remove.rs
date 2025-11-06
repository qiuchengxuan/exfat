use std::fmt::Debug;

use exfat::error::{Error, OperationError};

use crate::filepath::open;
use crate::types::{FileOrDirectory, Root};

pub fn remove<E: Debug, IO>(root: &mut Root<IO>, mut path: &str) -> Result<(), Error<E>>
where
    IO: exfat::io::IO<Error = E>,
{
    path = path.trim_end_matches('/');
    let (mut directory, name) = match path.rsplit_once('/') {
        Some((base, name)) => match open(root.open()?, &base)? {
            FileOrDirectory::File(_) => return Err(OperationError::NotDirectory.into()),
            FileOrDirectory::Directory(directory) => (directory, name),
        },
        None => (root.open()?, path),
    };
    let entryset = directory.find(name)?.ok_or(Error::Operation(OperationError::NotFound))?;
    directory.delete(&entryset)
}
