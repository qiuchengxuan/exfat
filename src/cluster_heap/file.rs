use super::entry::ClusterEntry;

pub struct File<IO> {
    pub(crate) entry: ClusterEntry<IO>,
    pub(crate) offset: u64,
}
