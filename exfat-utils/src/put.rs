use std::fs::File;
use std::io::Read;

use exfat::error::{Error, OperationError};
use exfat::{FileOrDirectory, RootDirectory as Root};

use crate::filepath::open;

pub fn put<E, IO>(root: &mut Root<E, IO>, mut path: &str, source: &str) -> Result<(), Error<E>>
where
    E: std::fmt::Debug,
    IO: exfat::io::IO<Error = E>,
{
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
