use std::fs::File;
use std::io;
use std::io::Read;

use exfat::error::{Error, OperationError};
use exfat::io::std::FileIO;
use exfat::ExFAT;
use exfat::FileOrDirectory;

use crate::filepath::open;

pub fn append(device: &str, path: &str, file: &str) -> Result<(), Error<io::Error>> {
    let metadata = std::fs::metadata(&file).expect("unable to read metadata");
    let mut file = File::open(&file).expect("No such file");
    let mut buffer = vec![0; metadata.len() as usize];
    file.read_to_end(&mut buffer).expect("Unable to read");

    let io = FileIO::open(device).map_err(|e| Error::IO(e))?;
    let mut exfat = ExFAT::new(io)?;
    exfat.validate_checksum()?;
    let mut root = exfat.root_directory()?;
    root.validate_upcase_table_checksum()?;
    let mut file = match open(root.open()?, &path)? {
        FileOrDirectory::File(f) => f,
        FileOrDirectory::Directory(_) => return Err(OperationError::NotFile.into()),
    };
    file.write_all(&buffer)?;
    Ok(())
}
