use core::fmt::{Debug, Display, Formatter, Result};

#[derive(displaydoc::Display)]
pub enum DataError {
    /// Not exFAT filesystem
    NotExFAT,
    /// Bad boot sector checksum
    BootChecksum,
    /// Allocation bitmap missing
    AllocationBitmapMissing,
    /// Upcase table missing
    UpcaseTableMissing,
    /// Bad upcase table checksum
    UpcaseTableChecksum,
    /// Broken FAT chain
    FATChain,
    /// Broken file or directory metadata
    Metadata,
}

#[derive(displaydoc::Display)]
pub enum ImplementationError {
    /// TexFAT not supported
    TexFATNotSupported,
    /// Create directory not supported
    CreateDirectoryNotSupported,
}

#[derive(displaydoc::Display)]
pub enum InputError {
    /// Name too long
    NameTooLong,
    /// Seek position out of range
    SeekPosition,
    /// Size out of range
    Size,
}

#[derive(displaydoc::Display)]
pub enum AllocationError {
    /// Allocation-not-possible is set in file metadata
    NotPossible,
    /// Need fragment while dont-fragment is set in file options
    Fragment,
    /// No more cluster available
    NoMoreCluster,
}

#[derive(displaydoc::Display)]
pub enum OperationError {
    /// File or directory already open
    AlreadyOpen,
    /// File or directory not found
    NotFound,
    /// Not a file
    NotFile,
    /// Not a directory
    NotDirectory,
    /// File or directory already exists
    AlreadyExists,
    /// Directory not empty when deleting
    DirectoryNotEmpty,
    /// End of file
    EOF,
}

pub enum Error<E> {
    IO(E),
    Data(DataError),
    Implementation(ImplementationError),
    Input(InputError),
    Operation(OperationError),
    Allocation(AllocationError),
}

impl<E: Debug> Debug for Error<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::IO(e) => write!(f, "IO({:?})", e),
            Self::Data(e) => write!(f, "{}", e),
            Self::Implementation(e) => write!(f, "{}", e),
            Self::Input(e) => write!(f, "{}", e),
            Self::Operation(e) => write!(f, "{}", e),
            Self::Allocation(e) => write!(f, "{}", e),
        }
    }
}

impl<E: Display> Display for Error<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::IO(e) => write!(f, "IO({})", e),
            Self::Data(e) => write!(f, "{}", e),
            Self::Implementation(e) => write!(f, "{}", e),
            Self::Input(e) => write!(f, "{}", e),
            Self::Operation(e) => write!(f, "{}", e),
            Self::Allocation(e) => write!(f, "{}", e),
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
from_error!(AllocationError, Allocation);
