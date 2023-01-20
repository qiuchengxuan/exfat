// Main boot region

use bitfield::bitfield;

use crate::endian::Little as LE;

bitfield! {
    #[derive(Copy, Clone, Debug, Default)]
    pub struct VolumeFlags(u16);
    pub media_failure, set_media_failure: 2, 2;
    pub volume_dirty, set_volume_dirty: 1, 1;
    pub active_fat, set_active_fat: 0, 0;
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub(crate) struct BootSector {
    pub jump_boot: [u8; 3],
    pub filesystem_name: [u8; 8],
    _padding: [u8; 53],
    pub partition_offset: LE<u64>, // shall ignore when 0
    pub volumn_length: LE<u64>,
    pub fat_offset: LE<u32>,          // unit sector
    pub fat_length: LE<u32>,          // unit sector
    pub cluster_heap_offset: LE<u32>, // unit sector
    pub cluster_count: LE<u32>,
    pub first_cluster_of_root_directory: LE<u32>,
    pub volumn_serial_number: LE<u32>,
    pub filesystem_revision: LE<u16>,
    pub volume_flags: LE<u16>,
    pub bytes_per_sector_shift: u8, // [9..=12]
    pub sectors_per_cluster_shift: u8,
    pub number_of_fats: u8,
    pub drive_select: u8,
    pub percent_inuse: u8,
    _reserved: [u8; 7],
    pub bootcode: [u8; 390],
    pub boot_signature: [u8; 2],
}

impl BootSector {
    pub fn is_exfat(&self) -> bool {
        self.jump_boot == hex!("EB 76 90") && &self.filesystem_name == b"EXFAT   "
    }

    pub fn volume_flags(&self) -> VolumeFlags {
        VolumeFlags(self.volume_flags.to_ne())
    }

    /// 512 ~ 4096
    pub fn bytes_per_sector(&self) -> u32 {
        2u32.pow(self.bytes_per_sector_shift as u32)
    }
}

#[derive(Default, Debug)]
pub(crate) struct BootChecksum(u32);

impl BootChecksum {
    pub fn write(&mut self, index: usize, sector: &[u8]) {
        let mut sum = self.0;
        for i in 0..sector.len() {
            match (index, i) {
                (0, 106 | 107 | 112) => continue,
                _ => sum = ((sum & 1) << 31) + (sum >> 1) + sector[i] as u32,
            }
        }
        self.0 = sum;
    }

    pub fn sum(&self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_exfat() {
        use std::io::Read;
        use std::process::Command as CMD;

        let args = ["-s", "4194304", "test.img"];
        let output = CMD::new("truncate").args(args).output().unwrap();
        assert!(output.status.success());
        let output = CMD::new("mkfs.exfat").args(["test.img"]).output().unwrap();
        assert!(output.status.success());

        let mut file = std::fs::File::open("test.img").unwrap();
        let mut bytes = [0u8; 512];
        file.read(&mut bytes).unwrap();
        let boot_sector: super::BootSector = unsafe { std::mem::transmute(bytes) };
        let mut checksum = super::BootChecksum::default();
        checksum.write(0, &bytes);
        for i in 1..11 {
            file.read(&mut bytes).unwrap();
            checksum.write(i, &bytes);
        }
        let mut bytes = [0u8; 4];
        file.read(&mut bytes).unwrap();
        CMD::new("rm").args(["-f", "test.img"]).output().unwrap();
        assert_eq!(u32::from_le_bytes(bytes), checksum.sum());
        println!("{:?} {:?}", boot_sector.jump_boot, boot_sector.filesystem_name);
        assert!(boot_sector.is_exfat());
    }
}
