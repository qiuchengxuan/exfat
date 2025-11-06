use std::fmt::Debug;

use exfat::error::Error;

use crate::filepath::open;
use crate::types::{FileOrDirectory, Root};

pub fn touch<E: Debug, IO>(root: &mut Root<IO>, path: &str) -> Result<(), Error<E>>
where
    IO: exfat::io::IO<Error = E>,
{
    let now = chrono::Utc::now();
    let directory = root.open()?;
    match open(directory, &path)? {
        FileOrDirectory::File(mut file) => file.touch(now.into(), Default::default()),
        FileOrDirectory::Directory(mut dir) => dir.touch(now.into(), Default::default()),
    }
}
