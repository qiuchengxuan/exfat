use displaydoc::Display;
use thiserror::Error;

#[derive(Debug, Display)]
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

impl core::error::Error for DataError {}

#[derive(Debug, Display)]
pub enum ImplementationError {
    /// TexFAT not supported
    TexFATNotSupported,
    /// Create directory not supported
    CreateDirectoryNotSupported,
}

impl core::error::Error for ImplementationError {}

#[derive(Debug, Display)]
pub enum InputError {
    /// Name too long
    NameTooLong,
    /// Seek position out of range
    SeekPosition,
    /// Size out of range
    Size,
}

impl core::error::Error for InputError {}

#[derive(Debug, Display)]
pub enum AllocationError {
    /// Allocation-not-possible is set in file metadata
    NotPossible,
    /// Need fragment while dont-fragment is set in file options
    Fragment,
    /// No more cluster available
    NoMoreCluster,
}

impl core::error::Error for AllocationError {}

#[derive(Debug, Display)]
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

impl core::error::Error for OperationError {}

#[derive(Error, Debug)]
pub enum Error<E> {
    #[error("IO({0:?})")]
    IO(E),
    #[error("{0:?}")]
    Data(#[from] DataError),
    #[error("{0:?}")]
    Implementation(#[from] ImplementationError),
    #[error("{0:?}")]
    Input(#[from] InputError),
    #[error("{0:?}")]
    Operation(#[from] OperationError),
    #[error("{0:?}")]
    Allocation(#[from] AllocationError),
}
