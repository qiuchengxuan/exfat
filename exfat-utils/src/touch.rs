use exfat::error::Error;
use exfat::{FileOrDirectory, RootDirectory as Root};

use super::filepath::open;

pub fn touch<E, IO>(root: &mut Root<E, IO>, path: &str) -> Result<(), Error<E>>
where
    E: std::fmt::Debug,
    IO: exfat::io::IO<Error = E>,
{
    let now = chrono::Utc::now();
    let directory = root.open()?;
    match open(directory, &path)? {
        FileOrDirectory::File(mut file) => file.touch(now.into(), Default::default()),
        FileOrDirectory::Directory(mut dir) => dir.touch(now.into(), Default::default()),
    }
}
