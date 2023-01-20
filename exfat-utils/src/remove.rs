use std::io;

use exfat::error::Error;
use exfat::io::std::FileIO;
use exfat::ExFAT;
use exfat::FileOrDirectory;

use crate::filepath::open;

pub fn remove(device: &str, mut path: &str) -> Result<(), Error<io::Error>> {
    let io = FileIO::open(device).map_err(|e| Error::IO(e))?;
    let mut exfat = ExFAT::new(io)?;
    exfat.validate_checksum()?;
    let mut root = exfat.root_directory()?;
    root.validate_upcase_table_checksum()?;
    path = path.trim_end_matches('/');
    match path.rsplit_once('/') {
        Some((base, name)) => {
            let mut directory = match open(root.open()?, &base)? {
                FileOrDirectory::File(_) => return Err(Error::InvalidInput("Not a directory")),
                FileOrDirectory::Directory(directory) => directory,
            };
            directory.delete(name)
        }
        None => root.open()?.delete(path),
    }
}
