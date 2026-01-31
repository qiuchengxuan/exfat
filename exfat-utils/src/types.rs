use std::{cell::RefCell, rc::Rc};

use exfat::RootDirectory;

pub type Shared<IO> = Rc<RefCell<IO>>;
pub type Root<IO> = RootDirectory<Shared<IO>>;
pub type Directory<E, IO> = exfat::Directory<E, IO, Shared<IO>>;
pub type FileOrDirectory<E, IO> = exfat::FileOrDirectory<E, IO, Shared<IO>>;
