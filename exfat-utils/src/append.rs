use std::fmt::Debug;
use std::fs::File;
use std::io::Read;

use exfat::SeekFrom;
use exfat::error::{Error, OperationError};

use crate::filepath::open;
use crate::types::{FileOrDirectory, Root};

pub fn append<E: Debug, IO>(root: &mut Root<IO>, path: &str, source: &str) -> Result<(), Error<E>>
where
    IO: exfat::io::IO<Error = E>,
{
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
