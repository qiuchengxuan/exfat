use std::{cell::RefCell, rc::Rc};

use exfat::RootDirectory;

pub type Shared<IO> = Rc<RefCell<IO>>;
pub type Root<IO> = RootDirectory<Shared<IO>>;
pub type Directory<IO> = exfat::Directory<Shared<IO>>;
pub type FileOrDirectory<IO> = exfat::FileOrDirectory<Shared<IO>>;
