#[derive(Copy, Clone, PartialEq)]
pub(crate) enum EntryType {
    AllocationBitmap,
    UpcaseTable,
    VolumnLabel,
    FileDirectory,
    VolumnGUID,
    TexFATPadding,
    StreamExtension,
    Filename,
    VendorExtension,
    VendorAllocation,
}

impl TryFrom<u8> for EntryType {
    type Error = u8;
    fn try_from(byte: u8) -> Result<Self, u8> {
        let value = match byte {
            // critical primary
            0x1 => Self::AllocationBitmap,
            0x2 => Self::UpcaseTable,
            0x3 => Self::VolumnLabel,
            0x5 => Self::FileDirectory,
            // benign primary
            0x20 => Self::VolumnGUID,
            0x21 => Self::TexFATPadding,
            // critical secondary
            0x40 => Self::StreamExtension,
            0x41 => Self::Filename,
            // benign secondary
            0x60 => Self::VendorExtension,
            0x61 => Self::VendorAllocation,
            _ => return Err(byte),
        };
        Ok(value)
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub(crate) struct RawEntryType(u8);

impl RawEntryType {
    pub(crate) fn new(byte: u8) -> Self {
        Self(byte)
    }

    pub(crate) fn in_use(&self) -> bool {
        self.0 & 0x80 > 0
    }

    pub(crate) fn entry_type(&self) -> Result<EntryType, u8> {
        EntryType::try_from(self.0 & 0x7F)
    }

    pub(crate) fn is_end_of_directory(&self) -> bool {
        self.0 == 0
    }
}
