use std::fmt::Debug;
use std::io;
use std::io::Write;

use exfat::error::{Error, OperationError};

use crate::filepath::open;
use crate::types::{FileOrDirectory, Root};

pub fn cat<E: Debug, IO>(root: &mut Root<IO>, path: &str) -> Result<(), Error<E>>
where
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
