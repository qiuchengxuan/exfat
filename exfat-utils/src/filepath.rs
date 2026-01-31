use std::fmt::Debug;
use std::ops::Deref;

use exfat::error::{Error, OperationError};
use exfat::io::Block;

use crate::types::{Directory, FileOrDirectory as FD};

const NOT_FOUND: OperationError = OperationError::NotFound;

pub fn open<B, E, IO>(mut dir: Directory<E, IO>, path: &str) -> Result<FD<E, IO>, Error<E>>
where
    B: Deref<Target = [Block]>,
    E: Debug,
    IO: for<'a> exfat::io::IO<Block<'a> = B, Error = E>,
{
    let path = path.trim().trim_matches('/');
    if path == "" {
        return Ok(FD::Directory(dir));
    }
    if let Some((parent, _)) = path.rsplit_once('/') {
        for name in parent.split('/') {
            let entryset = dir.find(name)?.ok_or(Error::Operation(NOT_FOUND))?;
            dir = match dir.open(&entryset)? {
                FD::Directory(dir) => dir,
                FD::File(_) => return Err(Error::Operation(NOT_FOUND)),
            }
        }
    }
    let name = path.rsplit_once('/').map(|(_, name)| name).unwrap_or(path);
    let entryset = dir.find(name)?.ok_or(Error::Operation(NOT_FOUND))?;
    dir.open(&entryset)
}
