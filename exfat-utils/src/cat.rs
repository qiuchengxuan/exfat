use std::io;
use std::io::Write;

use exfat::error::{Error, OperationError};
use exfat::io::std::FileIO;
use exfat::{FileOrDirectory, RootDirectory};

use crate::filepath::open;

type RootDir = RootDirectory<io::Error, FileIO>;

pub fn cat(root: &mut RootDir, path: &str) -> Result<(), Error<io::Error>> {
    let mut file = match open(root.open()?, &path)? {
        FileOrDirectory::File(f) => f,
        FileOrDirectory::Directory(_) => return Err(OperationError::NotFile.into()),
    };
    if file.size() == 0 {
        return Ok(());
    }
    let mut stdout = io::stdout();
    let mut buf = [0u8; 512];
    loop {
        let size = file.read(&mut buf)?;
        stdout.write_all(&buf[..size]).unwrap();
        if size < buf.len() {
            break;
        }
    }
    Ok(())
}
