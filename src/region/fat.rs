pub(crate) const FAT_ENTRY0: u32 = 0xF8FFFFFF;
pub(crate) const FAT_ENTRY1: u32 = 0xFFFFFFFF;

pub(crate) enum Entry {
    Next(u32),
    BadCluster,
    Last,
}

impl TryFrom<u32> for Entry {
    type Error = u32;
    fn try_from(value: u32) -> Result<Self, u32> {
        match value {
            2..=0xFFFFFFF6 => Ok(Self::Next(value)),
            0xFFFFFFF7 => Ok(Self::BadCluster),
            0xFFFFFFFF => Ok(Self::Last),
            _ => Err(value),
        }
    }
}
