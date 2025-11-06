use std::fmt::Debug;
use std::fs::File;
use std::io::Read;

use exfat::error::{Error, OperationError};

use crate::filepath::open;
use crate::types::{FileOrDirectory, Root};

pub fn put<E: Debug, IO>(root: &mut Root<IO>, path: &str, source: &str) -> Result<(), Error<E>>
where
    IO: exfat::io::IO<Error = E>,
{
    let path = path.trim_end_matches('/');
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
