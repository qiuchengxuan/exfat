use std::io;

use exfat::error::{Error, OperationError};
use exfat::io::std::FileIO;
use exfat::{FileOrDirectory, RootDirectory};

use crate::filepath::open;

type RootDir = RootDirectory<io::Error, FileIO>;

pub fn truncate(root: &mut RootDir, path: &str, size: u64) -> Result<(), Error<io::Error>> {
    let mut file = match open(root.open()?, &path)? {
        FileOrDirectory::File(f) => f,
        FileOrDirectory::Directory(_) => return Err(OperationError::NotFile.into()),
    };
    file.truncate(size)
}
