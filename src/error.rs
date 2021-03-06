use core::fmt::{Display, Formatter, Result};

pub enum Error<E> {
    IO(E),
    NotExFAT,
    Checksum,
    EOF,
    // FileDirectory
    UpcaseTableMissing,
    UpcaseTableChecksum,
    AllocationBitmapMissing,
    NoSuchFileOrDirectory,
}

impl<E: Display> Display for Error<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::IO(e) => write!(f, "IO({})", e),
            Self::NotExFAT => write!(f, "Not ExFAT filesystem"),
            Self::Checksum => write!(f, "Checksum mismatch"),
            Self::EOF => write!(f, "End of file"),
            Self::UpcaseTableMissing => write!(f, "Upcase table missing"),
            Self::UpcaseTableChecksum => write!(f, "Upcase table checksum mismatch"),
            Self::AllocationBitmapMissing => write!(f, "Allocation bitmap missing"),
            Self::NoSuchFileOrDirectory => write!(f, "No such file or directory"),
        }
    }
}
