use std::io;

use exfat::error::Error;
use exfat::io::std::FileIO;
use exfat::ExFAT;
use exfat::FileOrDirectory;

pub fn touch(device: String, path: String) -> Result<(), Error<io::Error>> {
    let io = FileIO::open(device).map_err(|e| Error::IO(e))?;
    let mut exfat = ExFAT::new(io)?;
    exfat.validate_checksum()?;
    let mut root = exfat.root_directory()?;
    root.validate_upcase_table_checksum()?;
    let now = chrono::Utc::now();
    match root.directory().open(path.as_str())? {
        FileOrDirectory::File(mut file) => file.touch(now.into(), Default::default()),
        FileOrDirectory::Directory(mut dir) => dir.touch(now.into(), Default::default()),
    }
}
