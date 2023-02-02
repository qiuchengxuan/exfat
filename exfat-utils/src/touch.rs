use std::io;

use exfat::error::Error;
use exfat::io::std::FileIO;
use exfat::{FileOrDirectory, RootDirectory};

use super::filepath::open;

type RootDir = RootDirectory<io::Error, FileIO>;

pub fn touch(root: &mut RootDir, path: &str) -> Result<(), Error<io::Error>> {
    let now = chrono::Utc::now();
    let directory = root.open()?;
    match open(directory, &path)? {
        FileOrDirectory::File(mut file) => file.touch(now.into(), Default::default()),
        FileOrDirectory::Directory(mut dir) => dir.touch(now.into(), Default::default()),
    }
}
