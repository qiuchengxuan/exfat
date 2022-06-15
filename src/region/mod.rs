/// Boot region, volume configuration parameters
/// 24 sectors, 12 sectors for main and 12 sectors for backup
pub(crate) mod boot;

/// FAT region, for only fragmented cluster chains
/// ([`fat-offset`][link] + [`fat-length`][link] * ([`num-fats`][link] - 1)) sectors
///
/// [link]: boot::BootSector
pub(crate) mod fat;

/// Data region
pub(crate) mod data;
