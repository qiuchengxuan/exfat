use std::fs::File;
use std::io;
use std::io::Read;

use exfat::error::{Error, OperationError};
use exfat::io::std::FileIO;
use exfat::{FileOrDirectory, RootDirectory};

use crate::filepath::open;

type RootDir = RootDirectory<io::Error, FileIO>;

pub fn put(root: &mut RootDir, mut path: &str, source: &str) -> Result<(), Error<io::Error>> {
    path = path.trim_end_matches('/');
    let (mut directory, name) = match path.rsplit_once('/') {
        Some((base, name)) => match open(root.open()?, &base)? {
            FileOrDirectory::File(_) => return Err(OperationError::NotDirectory.into()),
            FileOrDirectory::Directory(directory) => (directory, name),
        },
        None => (root.open()?, path),
    };
    if directory.find(name)?.is_some() {
        return Err(OperationError::AlreadyExists.into());
    }
    let mut source_file = File::open(&source).expect("No such file");
    let mut buffer = [0u8; 4096];
    directory.create(name, false)?;
    let entryset = directory.find(name)?.unwrap();
    let mut file = match directory.open(&entryset)? {
        FileOrDirectory::File(f) => f,
        FileOrDirectory::Directory(_) => unreachable!(),
    };
    loop {
        let size = source_file.read(&mut buffer).expect("Unable to read");
        if size == 0 {
            break;
        }
        file.write_all(&buffer[..size])?;
    }
    Ok(())
}
