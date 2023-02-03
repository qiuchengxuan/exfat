use std::io;

use exfat::error::{Error, OperationError};
use exfat::Directory;
use exfat::FileOrDirectory;

const NOT_FOUND: Error<io::Error> = Error::Operation(OperationError::NotFound);

pub fn open<IO>(
    mut dir: Directory<io::Error, IO>,
    path: &str,
) -> Result<FileOrDirectory<io::Error, IO>, Error<io::Error>>
where
    IO: exfat::io::IO<Error = io::Error>,
{
    let path = path.trim().trim_matches('/');
    if path == "" {
        return Ok(FileOrDirectory::Directory(dir));
    }
    if let Some((parent, _)) = path.rsplit_once('/') {
        for name in parent.split('/') {
            let entryset = dir.find(name)?.ok_or(NOT_FOUND)?;
            dir = match dir.open(&entryset)? {
                FileOrDirectory::Directory(dir) => dir,
                FileOrDirectory::File(_) => return Err(NOT_FOUND),
            }
        }
    }
    let name = path.rsplit_once('/').map(|(_, name)| name).unwrap_or(path);
    let entryset = dir.find(name)?.ok_or(NOT_FOUND)?;
    dir.open(&entryset)
}
