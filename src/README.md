embedded-exfat
==============

> An exFAT Library in rust mainly focusing on `no_std` embedded system with async support

`alloc` is mandatory for this crate, although memory allocation is minimized,
256B for upcase table and 12B plus name size for each file or directory,
and 12B for root directory.

For async scenario, enable `async-std` feature if std library available
otherwide enable `async` feature

For `no_std` scenario, be aware that thread safety is provided by spin crate,
which potenitally leads to dead lock.

Using this crate
----------------

```rust
use std::io::Write;
use exfat::error::Error;
use exfat::io::std::FileIO;
use exfat::ExFAT;
use exfat::FileOrDirectory;

let io = FileIO::open(device).map_err(|e| Error::IO(e)).unwrap();
let mut exfat = ExFAT::new(io).unwrap();
exfat.validate_checksum().unwrap();
let mut root = exfat.root_directory().unwrap();
root.validate_upcase_table_checksum().unwrap();
let mut file = match root.open().unwrap().open("test.txt").unwrap() {
    FileOrDirectory::File(f) => f,
    FileOrDirectory::Directory(_) => panic!("Not a file"),
};
let mut stdout = std::io::stdout();
let mut buf = [0u8; 512];
loop {
    let size = file.read(&mut buf).unwrap();
    stdout.write_all(&buf[..size]).unwrap();
    if size < buf.len() {
        break;
    }
}
```
