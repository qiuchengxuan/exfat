use std::io;

use exfat::error::{Error, OperationError};
use exfat::io::std::FileIO;
use exfat::ExFAT;
use exfat::FileOrDirectory;

use crate::filepath::open;

const NOT_FOUND: Error<io::Error> = Error::Operation(OperationError::NotFound);

pub fn remove(device: &str, mut path: &str) -> Result<(), Error<io::Error>> {
    let io = FileIO::open(device).map_err(|e| Error::IO(e))?;
    let mut exfat = ExFAT::new(io)?;
    exfat.validate_checksum()?;
    let mut root = exfat.root_directory()?;
    root.validate_upcase_table_checksum()?;
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
