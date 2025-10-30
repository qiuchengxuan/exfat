use std::mem::{MaybeUninit, transmute};

use derive_more::Display;
use exfat::io::Block;
use exfat::types::SectorID;
use mbr_nostd::{MasterBootRecord, PartitionTable};
use sdmmc::SD;
use sdmmc::bus::linux::{GPIO, IOError, SPI, SystemClock};
use sdmmc::bus::spi::bus;
use sdmmc::bus::spi::{BUSError, Bus};
use sdmmc::delay::std::Delay;
use spidev::SpidevOptions;
use thiserror::Error;

pub struct SDMMC {
    sd: SD<Bus<SPI, GPIO, SystemClock>>,
    offset: u32, // unit block
    num_blocks: u64,
    block_size_shift: u8,
    sector_size_shift: u8,
    address: u32,
    buffer: MaybeUninit<[u8; 4096]>,
    dirty: bool,
}

impl exfat::io::IO for SDMMC {
    type Block = Vec<Block>;
    type Error = BUSError<std::io::Error, IOError>;

    fn set_sector_size_shift(&mut self, shift: u8) -> Result<(), Self::Error> {
        if !(self.block_size_shift <= shift && shift <= 12) {
            panic!("Sector size out of range")
        }
        self.sector_size_shift = shift;
        Ok(())
    }

    fn read(&mut self, id: SectorID) -> Result<Vec<Block>, Self::Error> {
        let length = 1 << (self.sector_size_shift - self.block_size_shift);
        let address = u64::from(id) * length as u64;
        if address > self.num_blocks {
            panic!("Address out of range")
        }
        if self.address != address as u32 && self.dirty {
            self.flush()?;
        }
        self.address = address as u32;
        let mut buf = Vec::with_capacity(length);
        self.sd.read(self.offset + self.address, buf.iter_mut())?;
        Ok(buf)
    }

    fn write(&mut self, id: SectorID, offset: usize, data: &[u8]) -> Result<(), Self::Error> {
        let length = 1 << (self.sector_size_shift - self.block_size_shift);
        let address = u64::from(id) * length as u64;
        if address > self.num_blocks {
            panic!("Address out of range")
        }
        if self.address != address as u32 {
            self.flush()?;
            self.read(id)?;
            self.address = address as u32;
        }
        let sector = unsafe { self.buffer.assume_init_mut() };
        sector[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        if self.dirty {
            let sector = unsafe { self.buffer.assume_init_mut() };
            let blocks: &[[u8; 512]; 8] = unsafe { transmute(sector) };
            let length = 1 << (self.sector_size_shift - self.block_size_shift);
            self.sd.write(self.address, blocks[..length].iter())?;
            self.dirty = false;
        }
        Ok(())
    }
}

#[derive(Debug, Display, Error)]
pub enum Error {
    #[display("SDMMC: {_0:?}")]
    SDMMC(#[from] BUSError<std::io::Error, IOError>),
    #[display("{_0}")]
    String(&'static str),
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::SDMMC(BUSError::BUS(bus::Error::SPI(error)))
    }
}

impl SDMMC {
    pub fn new(spi: &str, cs: u16) -> Result<Self, Error> {
        let mut bus = sdmmc::bus::linux::spi(spi, cs)?;
        let card = bus.init(Delay)?;
        let mut sd = SD::init(bus, card)?;
        let options = SpidevOptions { max_speed_hz: Some(2_000_000), ..Default::default() };
        sd.bus(|bus| bus.spi(|spi| spi.0.configure(&options)))?;
        let offset = 0;
        let block_size_shift = sd.block_size_shift();
        let num_blocks: u64 = sd.num_blocks().into();
        let sdmmc = Self {
            sd,
            offset,
            num_blocks,
            block_size_shift,
            sector_size_shift: 9,
            address: u32::MAX,
            buffer: MaybeUninit::uninit(),
            dirty: false,
        };
        Ok(sdmmc)
    }

    pub fn set_patition(&mut self, partition: usize) -> Result<(), Error> {
        let mut buffer = [0u8; 512];
        self.sd.read(0, std::slice::from_mut(&mut buffer).iter_mut())?;
        let mbr = MasterBootRecord::from_bytes(&buffer).map_err(|_| Error::String("Not MBR"))?;
        let entries = mbr.partition_table_entries();
        let entry = entries.get(partition).ok_or(Error::String("Partition out of range"))?;
        if entry.sector_count == 0 {
            return Err(Error::String("Invalid partition"));
        }
        self.offset = entry.logical_block_address;
        self.num_blocks = entry.sector_count as u64;
        trace!("Partition offset {} num-blocks {}", self.offset, self.num_blocks);
        Ok(())
    }
}
