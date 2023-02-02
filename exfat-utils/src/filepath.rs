use std::io;

use exfat::error::{Error, OperationError};
use exfat::io::std::FileIO;
use exfat::Directory;
use exfat::FileOrDirectory;

const NOT_FOUND: Error<io::Error> = Error::Operation(OperationError::NotFound);

type Dir = Directory<io::Error, FileIO>;
type FileOrDir = FileOrDirectory<io::Error, FileIO>;

pub fn open(mut dir: Dir, path: &str) -> Result<FileOrDir, Error<io::Error>> {
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
