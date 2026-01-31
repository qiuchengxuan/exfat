use std::fmt::Debug;
use std::ops::Deref;

use exfat::error::Error;
use exfat::io::Block;

use crate::filepath::open;
use crate::types::{FileOrDirectory, Root};

pub fn touch<B, E: Debug, IO>(root: &mut Root<IO>, path: &str) -> Result<(), Error<E>>
where
    B: Deref<Target = [Block]>,
    IO: for<'a> exfat::io::IO<Block<'a> = B, Error = E>,
{
    let now = chrono::Utc::now();
    let directory = root.open()?;
    match open(directory, &path)? {
        FileOrDirectory::File(mut file) => file.touch(now.into(), Default::default()),
        FileOrDirectory::Directory(mut dir) => dir.touch(now.into(), Default::default()),
    }
}
