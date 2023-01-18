use core::fmt::{Display, Formatter, Result};

use crate::types::ClusterID;

pub enum Error<E> {
    Generic(&'static str),
    IO(E),
    NotExFAT,
    Checksum,
    EOF,
    NoSpace,
    BadCluster(ClusterID),
    // FAT
    TexFATNotSupported,
    // FileDirectory
    UpcaseTableMissing,
    UpcaseTableChecksum,
    AllocationBitmapMissing,
    NoSuchFileOrDirectory,
    AlreadyOpen,
    InvalidInput,
}

impl<E: Display> Display for Error<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::Generic(s) => write!(f, "Generic error({})", s),
            Self::IO(e) => write!(f, "IO({})", e),
            Self::NotExFAT => write!(f, "Not ExFAT filesystem"),
            Self::TexFATNotSupported => write!(f, "TexFAT not supported"),
            Self::Checksum => write!(f, "Checksum mismatch"),
            Self::EOF => write!(f, "End of file"),
            Self::NoSpace => write!(f, "Insufficent space"),
            Self::BadCluster(id) => write!(f, "Bad cluster({:#X})", u32::from(*id)),
            Self::UpcaseTableMissing => write!(f, "Upcase table missing"),
            Self::UpcaseTableChecksum => write!(f, "Upcase table checksum mismatch"),
            Self::AllocationBitmapMissing => write!(f, "Allocation bitmap missing"),
            Self::NoSuchFileOrDirectory => write!(f, "No such file or directory"),
            Self::AlreadyOpen => write!(f, "File or directory already open"),
            Self::InvalidInput => write!(f, "Invalid input"),
        }
    }
}
