use bitfield::bitfield;
#[cfg(all(feature = "chrono", feature = "std"))]
use chrono::Local;
#[cfg(feature = "chrono")]
use chrono::{Datelike, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use derive_more::Into;

use super::super::entry_type::{EntryType, RawEntryType};
use crate::endian::Little as LE;

bitfield! {
    #[derive(Copy, Clone, Debug, Default, Into)]
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
impl Into<NaiveDateTime> for Timestamp {
    fn into(self) -> NaiveDateTime {
        let date = NaiveDate::from_ymd_opt(self.year() as i32, self.month(), self.day());
        let time = NaiveTime::from_hms_opt(self.hour(), self.minute(), self.second());
        NaiveDateTime::new(date.unwrap_or_default(), time.unwrap_or_default())
    }
}

#[cfg(feature = "chrono")]
impl From<NaiveDateTime> for Timestamp {
    fn from(datetime: NaiveDateTime) -> Self {
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

#[cfg(all(feature = "chrono", feature = "std"))]
impl Timestamp {
    fn chrono_with_millis(&self, millis: u32) -> Result<NaiveDateTime, ()> {
        let date = NaiveDate::from_ymd_opt(self.year() as i32, self.month(), self.day());
        let (hour, minute, second) = (self.hour(), self.minute(), self.second());
        let time = NaiveTime::from_hms_milli_opt(hour, minute, second, millis);
        Ok(NaiveDateTime::new(date.ok_or(())?, time.ok_or(())?))
    }
}

bitfield! {
    #[derive(Copy, Clone, Default, Debug, Into)]
    pub struct FileAttributes(u16);
    pub read_only, set_read_only: 0, 0;
    pub hidden, set_hidden: 1, 1;
    pub system, set_system: 2, 2;
    pub directory, set_directory: 4, 4;
    pub archive, set_archive: 5, 5;
}

impl FileAttributes {
    pub fn new(directory: bool) -> Self {
        let mut attributes = Self::default();
        if directory {
            attributes.set_directory(1);
        } else {
            attributes.set_archive(1);
        }
        attributes
    }
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
impl core::convert::TryInto<FixedOffset> for UTCOffset {
    type Error = ();
    fn try_into(self) -> Result<FixedOffset, Self::Error> {
        FixedOffset::east_opt(self.minutes() as i32 * 60).ok_or(())
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct DateTime {
    pub timestamp: Timestamp,
    pub millisecond: u16,
    pub utc_offset: UTCOffset,
}

#[cfg(feature = "extern-datetime-now")]
unsafe extern "Rust" {
    pub(crate) fn exfat_datetime_now() -> DateTime;
}

impl DateTime {
    pub fn now() -> Self {
        match () {
            #[cfg(feature = "extern-datetime-now")]
            () => unsafe { exfat_datetime_now() },
            #[cfg(not(feature = "extern-datetime-now"))]
            () => Self::default(),
        }
    }
}

#[cfg(all(feature = "chrono", feature = "std"))]
impl DateTime {
    pub fn localtime(&self) -> Result<chrono::DateTime<Local>, ()> {
        let naive = self.timestamp.chrono_with_millis(self.millisecond as u32)?;
        let offset: FixedOffset = self.utc_offset.try_into()?;
        let datetime: chrono::DateTime<FixedOffset> =
            chrono::DateTime::from_naive_utc_and_offset(naive, offset);
        Ok(datetime.with_timezone(&Local))
    }
}

#[cfg(feature = "chrono")]
impl<TZ: chrono::Offset + chrono::TimeZone> From<chrono::DateTime<TZ>> for DateTime {
    fn from(datetime: chrono::DateTime<TZ>) -> Self {
        let offset = datetime.timezone().fix();
        let seconds = offset.local_minus_utc();
        let utc_offset = UTCOffset::new((seconds / 60) as i16);
        let naive = datetime.naive_utc();
        let millisecond = naive.and_utc().timestamp_subsec_millis() as u16;
        Self { timestamp: naive.into(), millisecond, utc_offset }
    }
}

#[derive(Copy, Clone, Default, Debug)]
#[repr(C, packed(1))]
pub struct FileDirectory {
    pub(crate) entry_type: RawEntryType,
    pub(crate) secondary_count: u8,
    pub(crate) set_checksum: LE<u16>,
    pub(crate) file_attributes: LE<u16>,
    _reserved1: [u8; 2],
    create_timestamp: LE<u32>,
    last_modified_timestamp: LE<u32>,
    last_accessed_timestamp: LE<u32>,
    create_10ms_increment: u8,
    last_modified_10ms_increment: u8,
    create_utc_offset: UTCOffset,
    last_modified_utc_offset: UTCOffset,
    last_accessed_utc_offset: UTCOffset,
    _reserved2: [u8; 7],
}

impl FileDirectory {
    pub(crate) fn new(secondary_count: u8, directory: bool) -> Self {
        let now = DateTime::now();
        let timestamp: LE<u32> = u32::from(now.timestamp).into();
        let millis = (now.millisecond / 10) as u8;
        FileDirectory {
            entry_type: RawEntryType::new(EntryType::FileDirectory, true),
            secondary_count,
            file_attributes: u16::from(FileAttributes::new(directory)).into(),
            create_timestamp: timestamp,
            create_10ms_increment: millis,
            create_utc_offset: now.utc_offset,
            last_modified_timestamp: timestamp,
            last_modified_10ms_increment: millis,
            last_modified_utc_offset: now.utc_offset,
            last_accessed_timestamp: timestamp,
            last_accessed_utc_offset: now.utc_offset,
            ..Default::default()
        }
    }

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

pub(crate) struct Checksum(u16);

impl Checksum {
    pub(crate) fn new() -> Self {
        Self(0)
    }

    pub(crate) fn write(&mut self, value: u16) {
        self.0 = if self.0 & 1 > 0 { 0x8000 } else { 0 } + (self.0 >> 1) + value
    }

    pub(crate) fn sum(&self) -> u16 {
        self.0
    }
}

pub(crate) fn name_hash(name: &str) -> u16 {
    let mut checksum = Checksum::new();
    for ch in name.chars() {
        checksum.write(ch as u16);
        checksum.write(((ch as u32) >> 16) as u16);
    }
    checksum.sum()
}
