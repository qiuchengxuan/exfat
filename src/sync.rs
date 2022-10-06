#[cfg(feature = "alloc")]
pub(crate) use alloc::sync::Arc;
#[cfg(feature = "std")]
pub(crate) use std::sync::Arc;

#[cfg(all(feature = "async", feature = "std"))]
pub(crate) use async_std::sync::Mutex;
#[cfg(all(feature = "async", not(feature = "std")))]
pub(crate) use fast_async_mutex::mutex::Mutex;
#[cfg(all(not(feature = "async"), feature = "std"))]
pub(crate) use std::sync::Mutex;
