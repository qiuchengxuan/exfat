use std::io;

use exfat::error::Error;
use exfat::io::std::FileIO;
use exfat::ExFAT;

pub fn list(device: String, _path: String) -> Result<(), Error<io::Error>> {
    let io = FileIO::open(device).map_err(|e| Error::IO(e))?;
    let mut exfat = ExFAT::new(io)?;
    exfat.validate_checksum()?;
    let mut root = exfat.root_directory()?;
    root.validate_upcase_table_checksum()?;
    root.directory().find(|entry_set| -> Option<()> {
        let attrs = entry_set.file_directory.file_attributes();
        print!("{}", if attrs.directory() > 0 { "d" } else { "-" });
        print!("{}", if attrs.read_only() > 0 { "r" } else { "-" });
        print!("{}", if attrs.system() > 0 { "s" } else { "-" });
        print!("{}", if attrs.hidden() > 0 { "h" } else { "-" });
        print!("{}", if attrs.archive() > 0 { "a" } else { "-" });
        print!(" {:8}", entry_set.stream_extension.data_length());
        let create_at = entry_set.file_directory.create_timestamp();
        let localtime = create_at.localtime().unwrap();
        print!(" {}", localtime.format("%Y-%m-%d %H:%M:%S"));
        if attrs.directory() > 0 {
            println!(" {}/", entry_set.name);
        } else {
            println!(" {}", entry_set.name);
        }
        None
    })?;
    Ok(())
}
