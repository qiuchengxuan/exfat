use std::ops::Deref;

use exfat::Directory as Dir;
use exfat::FileOrDirectory as FileOrDir;
use exfat::error::{Error, OperationError};
use exfat::io::Block;

const NOT_FOUND: OperationError = OperationError::NotFound;

pub fn open<B, E, IO>(mut dir: Dir<B, E, IO>, path: &str) -> Result<FileOrDir<B, E, IO>, Error<E>>
where
    B: Deref<Target = [Block]>,
    E: std::fmt::Debug,
    IO: exfat::io::IO<Block = B, Error = E>,
{
    let path = path.trim().trim_matches('/');
    if path == "" {
        return Ok(FileOrDir::Directory(dir));
    }
    if let Some((parent, _)) = path.rsplit_once('/') {
        for name in parent.split('/') {
            let entryset = dir.find(name)?.ok_or(Error::Operation(NOT_FOUND))?;
            dir = match dir.open(&entryset)? {
                FileOrDir::Directory(dir) => dir,
                FileOrDir::File(_) => return Err(Error::Operation(NOT_FOUND)),
            }
        }
    }
    let name = path.rsplit_once('/').map(|(_, name)| name).unwrap_or(path);
    let entryset = dir.find(name)?.ok_or(Error::Operation(NOT_FOUND))?;
    dir.open(&entryset)
}
