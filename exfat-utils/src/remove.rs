use std::fmt::Debug;
use std::ops::Deref;

use exfat::error::{Error, OperationError};
use exfat::io::Block;

use crate::filepath::open;
use crate::types::{FileOrDirectory, Root};

pub fn remove<B, E: Debug, IO>(root: &mut Root<IO>, mut path: &str) -> Result<(), Error<E>>
where
    B: Deref<Target = [Block]>,
    IO: for<'a> exfat::io::IO<Block<'a> = B, Error = E>,
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
