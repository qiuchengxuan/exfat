pub(crate) mod primary;
pub(crate) mod secondary;

pub(crate) const ENTRY_SIZE: usize = 32;
pub(crate) type RawEntry = [u8; ENTRY_SIZE];
