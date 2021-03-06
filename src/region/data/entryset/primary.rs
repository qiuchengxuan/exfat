use bitfield::bitfield;
#[cfg(feature = "chrono")]
use chrono::{Datelike, Timelike};

use super::super::entry_type::RawEntryType;
use crate::endian::Little as LE;

bitfield! {
    #[derive(Copy, Clone, Debug, Default)]
    pub struct Timestamp(u32);
    year_offset, set_year_offset: 31, 25;
    pub month, set_month: 24, 21;
    pub day, set_day: 20, 16;
    pub hour, set_hour: 15, 11;
    pub minute, set_minute: 10, 5;
    pub double_second, set_double_second: 4, 0;
}

impl Timestamp {
    pub fn year(&self) -> u32 {
        self.year_offset() + 1980
    }

    pub fn set_year(&mut self, year: u32) {
        self.set_year_offset(year - 1980)
    }

    pub fn second(&self) -> u32 {
        self.double_second() * 2
    }

    pub fn set_second(&mut self, second: u32) {
        self.set_double_second(second / 2)
    }
}

#[cfg(feature = "chrono")]
impl Into<chrono::NaiveDateTime> for Timestamp {
    fn into(self) -> chrono::NaiveDateTime {
        let date = chrono::NaiveDate::from_ymd(self.year() as i32, self.month(), self.day());
        let time = chrono::NaiveTime::from_hms(self.hour(), self.minute(), self.second());
        chrono::NaiveDateTime::new(date, time)
    }
}

#[cfg(feature = "chrono")]
impl From<chrono::NaiveDateTime> for Timestamp {
    fn from(datetime: chrono::NaiveDateTime) -> Self {
        let mut timestamp = Self::default();
        timestamp.set_year(datetime.year() as u32);
        timestamp.set_month(datetime.month());
        timestamp.set_day(datetime.day());
        timestamp.set_hour(datetime.hour());
        timestamp.set_minute(datetime.minute());
        timestamp.set_second(datetime.second());
        timestamp
    }
}

#[cfg(feature = "chrono")]
impl Timestamp {
    fn chrono_with_millis(&self, millis: u32) -> chrono::NaiveDateTime {
        let date = chrono::NaiveDate::from_ymd(self.year() as i32, self.month(), self.day());
        let time =
            chrono::NaiveTime::from_hms_milli(self.hour(), self.minute(), self.second(), millis);
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
    pub fn new(minutes: i16) -> Self {
        Self((minutes / 15) as u8 | 0x80)
    }

    pub fn minutes(&self) -> i16 {
        match self.0 & 0x80 > 0 {
            true => (((self.0 & 0x7F) << 1) as i8 >> 1) as i16 * 15,
            false => 0,
        }
    }
}

#[cfg(feature = "chrono")]
impl Into<chrono::FixedOffset> for UTCOffset {
    fn into(self) -> chrono::FixedOffset {
        chrono::FixedOffset::east(self.minutes() as i32 * 60)
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct DateTime {
    pub timestamp: Timestamp,
    pub millisecond: u16,
    pub utc_offset: UTCOffset,
}

#[cfg(feature = "chrono")]
impl Into<chrono::DateTime<chrono::FixedOffset>> for DateTime {
    fn into(self) -> chrono::DateTime<chrono::FixedOffset> {
        let datetime = self.timestamp.chrono_with_millis(self.millisecond as u32);
        chrono::DateTime::from_utc(datetime, self.utc_offset.into())
    }
}

#[cfg(feature = "chrono")]
impl<TZ: chrono::Offset + chrono::TimeZone> From<chrono::DateTime<TZ>> for DateTime {
    fn from(datetime: chrono::DateTime<TZ>) -> Self {
        let offset = datetime.timezone().fix();
        let seconds = offset.local_minus_utc();
        let utc_offset = UTCOffset::new((seconds / 60) as i16);
        let naive = datetime.naive_utc();
        let millisecond = naive.timestamp_subsec_millis() as u16;
        Self {
            timestamp: naive.into(),
            millisecond,
            utc_offset,
        }
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

    pub fn create_timestamp(&self) -> DateTime {
        DateTime {
            timestamp: Timestamp(self.create_timestamp.to_ne()),
            millisecond: self.create_10ms_increment as u16 * 10,
            utc_offset: self.create_utc_offset,
        }
    }

    pub fn last_modified_timestamp(&self) -> DateTime {
        DateTime {
            timestamp: Timestamp(self.last_modified_timestamp.to_ne()),
            millisecond: self.last_modified_10ms_increment as u16 * 10,
            utc_offset: self.last_modified_utc_offset,
        }
    }

    pub(crate) fn update_last_modified_timestamp(&mut self, datetime: DateTime) {
        self.last_modified_timestamp = datetime.timestamp.0.into();
        self.last_modified_10ms_increment = (datetime.millisecond / 10) as u8;
        self.last_modified_utc_offset = datetime.utc_offset;
    }

    pub fn last_accessed_timestamp(&self) -> DateTime {
        DateTime {
            timestamp: Timestamp(self.last_accessed_timestamp.to_ne()),
            millisecond: 0,
            utc_offset: self.last_accessed_utc_offset,
        }
    }

    pub(crate) fn update_last_accessed_timestamp(&mut self, datetime: DateTime) {
        self.last_accessed_timestamp = datetime.timestamp.0.into();
        self.last_accessed_utc_offset = datetime.utc_offset;
    }
}
