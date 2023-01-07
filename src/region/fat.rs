use crate::types::ClusterID;

#[derive(Copy, Clone, Debug)]
pub(crate) enum Entry {
    Next(ClusterID),
    BadCluster,
    Last,
}

impl TryFrom<u32> for Entry {
    type Error = u32;
    fn try_from(value: u32) -> Result<Self, u32> {
        match value {
            2..=0xFFFFFFF6 => Ok(Self::Next(value.into())),
            0xFFFFFFF7 => Ok(Self::BadCluster),
            0xFFFFFFFF => Ok(Self::Last),
            _ => Err(value),
        }
    }
}
