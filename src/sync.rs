#[cfg(all(feature = "async", feature = "std"))]
pub(crate) use async_std::sync::Mutex;
#[cfg(not(feature = "std"))]
pub(crate) use spin::Mutex;
#[cfg(all(not(feature = "async"), feature = "std"))]
pub(crate) use std::sync::Mutex;
