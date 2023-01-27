use std::io;

use exfat::error::{Error, OperationError};
use exfat::Directory;
use exfat::FileOrDirectory;

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
            dir = match dir.open(name)? {
                FileOrDirectory::Directory(dir) => dir,
                FileOrDirectory::File(_) => return Err(OperationError::NotFound.into()),
            }
        }
    }
    dir.open(path.rsplit_once('/').map(|(_, name)| name).unwrap_or(path))
}
