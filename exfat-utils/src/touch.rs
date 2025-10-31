use std::ops::Deref;

use exfat::error::Error;
use exfat::io::Block;
use exfat::{FileOrDirectory, RootDirectory as Root};

use super::filepath::open;

pub fn touch<B, E, IO>(root: &mut Root<B, E, IO>, path: &str) -> Result<(), Error<E>>
where
    B: Deref<Target = [Block]>,
    E: std::fmt::Debug,
    IO: exfat::io::IO<Block = B, Error = E>,
{
    let now = chrono::Utc::now();
    let directory = root.open()?;
    match open(directory, &path)? {
        FileOrDirectory::File(mut file) => file.touch(now.into(), Default::default()),
        FileOrDirectory::Directory(mut dir) => dir.touch(now.into(), Default::default()),
    }
}
