use std::io;
use std::io::Write;

use exfat::error::Error;
use exfat::io::std::FileIO;
use exfat::ExFAT;
use exfat::FileOrDirectory;

pub fn cat(device: String, path: String) -> Result<(), Error<io::Error>> {
    let io = FileIO::open(device).map_err(|e| Error::IO(e))?;
    let mut exfat = ExFAT::new(io)?;
    exfat.validate_checksum()?;
    let mut root = exfat.root_directory()?;
    root.validate_upcase_table_checksum()?;
    let mut file = match root.directory().open(path.as_str())? {
        FileOrDirectory::File(f) => f,
        _ => return Err(Error::NoSuchFileOrDirectory),
    };
    let mut stdout = io::stdout();
    let mut buf = [0u8; 512];
    loop {
        let size = file.read(&mut buf)?;
        stdout.write_all(&buf[..size]).unwrap();
        if size < buf.len() {
            break;
        }
    }
    Ok(())
}
