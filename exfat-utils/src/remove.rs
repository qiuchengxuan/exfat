use std::io;

use exfat::error::{Error, OperationError};
use exfat::io::std::FileIO;
use exfat::{FileOrDirectory, RootDirectory};

use crate::filepath::open;

const NOT_FOUND: Error<io::Error> = Error::Operation(OperationError::NotFound);

type RootDir = RootDirectory<io::Error, FileIO>;

pub fn remove(root: &mut RootDir, mut path: &str) -> Result<(), Error<io::Error>> {
    path = path.trim_end_matches('/');
    let (mut directory, name) = match path.rsplit_once('/') {
        Some((base, name)) => match open(root.open()?, &base)? {
            FileOrDirectory::File(_) => return Err(OperationError::NotDirectory.into()),
            FileOrDirectory::Directory(directory) => (directory, name),
        },
        None => (root.open()?, path),
    };
    let entryset = directory.find(name)?.ok_or(NOT_FOUND)?;
    directory.delete(&entryset)
}
