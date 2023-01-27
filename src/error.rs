use core::fmt::{Debug, Formatter, Result};

#[derive(Debug)]
pub enum DataError {
    NotExFAT,
    BootChecksum,
    AllocationBitmapMissing,
    UpcaseTableMissing,
    UpcaseTableChecksum,
    FATChain,
    Metadata,
}

#[derive(Debug)]
pub enum ImplementationError {
    TexFATNotSupported,
    CreateDirectoryNotSupported,
}

#[derive(Debug)]
pub enum InputError {
    NameTooLong,
    SeekPosition,
    Size,
}

#[derive(Debug)]
pub enum OperationError {
    AlreadyOpen,
    NotFound,
    NotFile,
    NotDirectory,
    AlreadyExists,
    DirectoryNotEmpty,
    EOF,
    ClusterAllocation,
    AllocationNotPossible,
}

pub enum Error<E> {
    IO(E),
    Data(DataError),
    Implementation(ImplementationError),
    Input(InputError),
    Operation(OperationError),
}

impl<E: Debug> Debug for Error<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::IO(e) => write!(f, "IO({:?})", e),
            Self::Data(e) => write!(f, "{:?}", e),
            Self::Implementation(e) => write!(f, "{:?}", e),
            Self::Input(e) => write!(f, "{:?}", e),
            Self::Operation(e) => write!(f, "{:?}", e),
        }
    }
}

macro_rules! from_error {
    ($type:ty, $variant:ident) => {
        impl<E> From<$type> for Error<E> {
            fn from(e: $type) -> Self {
                Self::$variant(e)
            }
        }
    };
}

from_error!(DataError, Data);
from_error!(ImplementationError, Implementation);
from_error!(InputError, Input);
from_error!(OperationError, Operation);
