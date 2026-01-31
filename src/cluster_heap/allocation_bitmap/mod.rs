mod bitmap;
mod locator;
pub(crate) mod meta;

pub(crate) mod allocate;
pub(crate) mod allocator;
pub(crate) mod release;

pub use allocator::DumbAllocator as AllocationBitmap;
pub use meta::Meta;
