use bitfield::bitfield;
#[cfg(feature = "chrono")]
use chrono::Duration;

use super::super::entry_type::RawEntryType;
use crate::endian::Little as LE;

bitfield! {
    #[derive(Copy, Clone, Debug)]
    pub struct Timestamp(u32);
    _year, _set_year: 31, 25;
    pub month, set_month: 24, 21;
    pub day, set_day: 20, 16;
    pub hour, set_hour: 15, 11;
    pub minute, set_minute: 10, 5;
    pub double_seconds, set_double_seconds: 4, 0;
}

impl Timestamp {
    pub fn year(&self) -> u32 {
        self._year() + 1980
    }

    pub fn set_year(&mut self, year: u32) {
        self._set_year(year - 1980)
    }

    pub fn seconds(&self) -> u32 {
        self.double_seconds() * 2
    }

    pub fn set_seconds(&mut self, seconds: u32) {
        self.set_double_seconds(seconds / 2)
    }
}

#[cfg(feature = "chrono")]
impl Into<chrono::NaiveDateTime> for Timestamp {
    fn into(self) -> chrono::NaiveDateTime {
        let date = chrono::NaiveDate::from_ymd(self.year() as i32, self.month(), self.day());
        let time = chrono::NaiveTime::from_hms(self.hour(), self.minute(), self.seconds());
        chrono::NaiveDateTime::new(date, time)
    }
}

#[cfg(feature = "chrono")]
impl Timestamp {
    fn chrono_with_millis(&self, millis: u32) -> chrono::NaiveDateTime {
        let date = chrono::NaiveDate::from_ymd(self.year() as i32, self.month(), self.day());
        let time =
            chrono::NaiveTime::from_hms_milli(self.hour(), self.minute(), self.seconds(), millis);
        chrono::NaiveDateTime::new(date, time)
    }
}

bitfield! {
    #[derive(Copy, Clone, Debug)]
    pub struct FileAttributes(u16);
    pub read_only, set_read_only: 0, 0;
    pub hidden, set_hidden: 1, 1;
    pub system, set_system: 2, 2;
    pub directory, set_directory: 4, 4;
    pub archive, set_archive: 5, 5;
}

#[derive(Copy, Clone, Debug, Default)]
pub struct UTCOffset(u8);

impl UTCOffset {
    pub fn get(&self) -> Option<u8> {
        match self.0 & 0x80 > 0 {
            true => Some(self.0 & 0x7F),
            false => None,
        }
    }

    pub fn set(&mut self, offset: Option<u8>) {
        match offset {
            Some(offset) => self.0 = offset | 0x80,
            None => self.0 = 0,
        }
    }
}

#[cfg(feature = "chrono")]
impl Into<chrono::FixedOffset> for UTCOffset {
    fn into(self) -> chrono::FixedOffset {
        chrono::FixedOffset::east(self.get().unwrap_or_default() as i32 * 15 * 60)
    }
}

#[derive(Copy, Clone, Default, Debug)]
#[repr(C, packed(1))]
pub struct FileDirectory {
    pub(crate) entry_type: RawEntryType,
    pub(crate) secondary_count: u8,
    pub(crate) set_checksum: LE<u16>,
    file_attributes: LE<u16>,
    _reserved1: [u8; 2],
    create_timestamp: LE<u32>,
    last_modified_timestamp: LE<u32>,
    last_accessed_timestamp: LE<u32>,
    pub create_10ms_increment: u8,
    pub last_modified_10ms_increment: u8,
    pub create_utc_offset: UTCOffset,
    pub last_modified_utc_offset: UTCOffset,
    pub last_accessed_utc_offset: UTCOffset,
    _reserved2: [u8; 7],
}

impl FileDirectory {
    pub fn file_attributes(&self) -> FileAttributes {
        FileAttributes(self.file_attributes.to_ne())
    }

    pub fn create_timestamp(&self) -> Timestamp {
        Timestamp(self.create_timestamp.to_ne())
    }

    pub fn last_modified_timestamp(&self) -> Timestamp {
        Timestamp(self.last_modified_timestamp.to_ne())
    }

    pub fn last_accessed_timestamp(&self) -> Timestamp {
        Timestamp(self.last_accessed_timestamp.to_ne())
    }
}

#[cfg(feature = "chrono")]
impl FileDirectory {
    pub fn chrono_create_timestamp(&self) -> chrono::DateTime<chrono::FixedOffset> {
        let timestamp = Timestamp(self.create_timestamp.to_ne());
        let datetime: chrono::NaiveDateTime =
            timestamp.chrono_with_millis(self.create_10ms_increment as u32 * 10);
        chrono::DateTime::from_utc(datetime, self.create_utc_offset.into())
    }

    pub fn chrono_last_modified_timestamp(&self) -> chrono::DateTime<chrono::FixedOffset> {
        let timestamp = Timestamp(self.last_modified_timestamp.to_ne());
        let datetime: chrono::NaiveDateTime =
            timestamp.chrono_with_millis(self.last_modified_10ms_increment as u32 * 10);
        chrono::DateTime::from_utc(datetime, self.last_modified_utc_offset.into())
    }

    pub fn chrono_last_accessed_timestamp(&self) -> chrono::DateTime<chrono::FixedOffset> {
        let timestamp = Timestamp(self.last_accessed_timestamp.to_ne());
        let datetime: chrono::NaiveDateTime = timestamp.into();
        chrono::DateTime::from_utc(datetime, self.last_accessed_utc_offset.into())
    }
}
