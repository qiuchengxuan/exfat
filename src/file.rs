#[cfg(feature = "max-filename-size-64")]
pub const MAX_FILENAME_SIZE: usize = 64;
#[cfg(not(feature = "limit-filename-size"))]
pub const MAX_FILENAME_SIZE: usize = 510;

#[derive(Copy, Clone)]
pub struct TouchOptions {
    pub access: bool,
    pub modified: bool,
}

impl Default for TouchOptions {
    fn default() -> Self {
        Self { access: true, modified: true }
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct FileOptions {
    /// Fragment will produce unperdictable latency when writing,
    /// enabling this option will indicate write operation
    /// returns DontFragment error instead of allocating fragemnted cluster
    pub dont_fragment: bool,
}
