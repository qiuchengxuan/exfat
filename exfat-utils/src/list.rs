use std::io;

use exfat::error::{Error, OperationError};
use exfat::io::std::FileIO;
use exfat::{FileOrDirectory, RootDirectory};

use super::filepath::open;

type RootDir = RootDirectory<io::Error, FileIO>;

pub fn list(root: &mut RootDir, path: &str) -> Result<(), Error<io::Error>> {
    let mut directory = match open(root.open()?, &path)? {
        FileOrDirectory::File(_) => return Err(OperationError::NotDirectory.into()),
        FileOrDirectory::Directory(dir) => dir,
    };
    directory.walk(|entryset| -> bool {
        if !entryset.in_use() {
            return false;
        }
        let attrs = entryset.file_directory.file_attributes();
        print!("{}", if attrs.directory() > 0 { "d" } else { "-" });
        print!("{}", if attrs.read_only() > 0 { "r" } else { "-" });
        print!("{}", if attrs.system() > 0 { "s" } else { "-" });
        print!("{}", if attrs.hidden() > 0 { "h" } else { "-" });
        print!("{}", if attrs.archive() > 0 { "a" } else { "-" });
        print!(" {:8}", entryset.valid_data_length());
        let modified_at = entryset.file_directory.last_modified_timestamp();
        let localtime = modified_at.localtime().unwrap();
        print!(" {}", localtime.format("%Y-%m-%d %H:%M:%S"));
        if attrs.directory() > 0 {
            println!(" {}/", entryset.name());
        } else {
            println!(" {}", entryset.name());
        }
        false
    })?;
    Ok(())
}
