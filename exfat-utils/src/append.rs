use std::fs::File;
use std::io;
use std::io::Read;

use exfat::error::{Error, OperationError};
use exfat::io::std::FileIO;
use exfat::{FileOrDirectory, RootDirectory, SeekFrom};

use crate::filepath::open;

type RootDir = RootDirectory<io::Error, FileIO>;

pub fn append(root: &mut RootDir, path: &str, source: &str) -> Result<(), Error<io::Error>> {
    let mut source_file = File::open(&source).expect("No such file");
    let mut buffer = [0u8; 4096];
    let mut file = match open(root.open()?, &path)? {
        FileOrDirectory::File(f) => f,
        FileOrDirectory::Directory(_) => return Err(OperationError::NotFile.into()),
    };
    file.seek(SeekFrom::End(0))?;
    loop {
        let size = source_file.read(&mut buffer).expect("Unable to read");
        if size == 0 {
            break;
        }
        file.write_all(&buffer[..size])?;
    }
    Ok(())
}
