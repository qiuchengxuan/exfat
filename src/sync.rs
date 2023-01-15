#[cfg(all(feature = "async", feature = "std"))]
pub(crate) use async_std::sync::Mutex;
#[cfg(all(feature = "async", not(feature = "std")))]
pub(crate) use fast_async_mutex::mutex::Mutex;
#[cfg(all(not(feature = "async"), not(feature = "std")))]
pub(crate) use spin_sync::Mutex;
#[cfg(all(not(feature = "async"), feature = "std"))]
pub(crate) use std::sync::Mutex;
