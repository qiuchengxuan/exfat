use std::io;
use std::io::Write;

use exfat::error::{Error, OperationError};
use exfat::{FileOrDirectory, RootDirectory as Root};

use crate::filepath::open;

pub fn cat<E, IO>(root: &mut Root<E, IO>, path: &str) -> Result<(), Error<E>>
where
    E: std::fmt::Debug,
    IO: exfat::io::IO<Error = E>,
{
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
