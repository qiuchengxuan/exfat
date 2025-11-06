use std::fmt::Debug;

use exfat::error::{Error, OperationError};

use crate::types::{Directory, FileOrDirectory};

const NOT_FOUND: OperationError = OperationError::NotFound;

pub fn open<E: Debug, IO>(
    mut dir: Directory<IO>,
    path: &str,
) -> Result<FileOrDirectory<IO>, Error<E>>
where
    IO: exfat::io::IO<Error = E>,
{
    let path = path.trim().trim_matches('/');
    if path == "" {
        return Ok(FileOrDirectory::Directory(dir));
    }
    if let Some((parent, _)) = path.rsplit_once('/') {
        for name in parent.split('/') {
            let entryset = dir.find(name)?.ok_or(Error::Operation(NOT_FOUND))?;
            dir = match dir.open(&entryset)? {
                FileOrDirectory::Directory(dir) => dir,
                FileOrDirectory::File(_) => return Err(Error::Operation(NOT_FOUND)),
            }
        }
    }
    let name = path.rsplit_once('/').map(|(_, name)| name).unwrap_or(path);
    let entryset = dir.find(name)?.ok_or(Error::Operation(NOT_FOUND))?;
    dir.open(&entryset)
}
