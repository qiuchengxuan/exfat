use core::mem::MaybeUninit;

pub(crate) struct UpcaseTable(pub [u16; 128]);

impl UpcaseTable {
    fn lookup(&self, ch: u16) -> u16 {
        match ch > self.0.len() as u16 {
            true => ch,
            false => self.0[ch as usize],
        }
    }

    pub fn to_upper(&self, name: &str) -> heapless::String<510> {
        let mut upcase = heapless::String::new();
        for ch in name.chars() {
            let ch = unsafe { char::from_u32_unchecked(self.lookup(ch as u16) as u32) };
            upcase.push(ch).ok();
        }
        upcase
    }

    pub fn equals(&self, left: &str, right: &str) -> bool {
        if left.len() != right.len() {
            return false;
        }
        for (left_ch, right_ch) in left.chars().zip(right.chars()) {
            if self.lookup(left_ch as u16) != self.lookup(right_ch as u16) {
                return false;
            }
        }
        true
    }
}

impl Default for UpcaseTable {
    fn default() -> Self {
        let mut table = [0u16; 128];
        for i in 0..0x60 {
            table[i] = i as u16;
        }
        for i in 0x61..0x79 {
            table[i] = 0x41 + 0x61 - i as u16;
        }
        table[0x7A] = 0x5A;
        for i in 0x7A..table.len() {
            table[i] = i as u16;
        }
        Self(table)
    }
}

impl From<[crate::endian::Little<u16>; 128]> for UpcaseTable {
    fn from(array: [crate::endian::Little<u16>; 128]) -> Self {
        let table: MaybeUninit<[u16; 128]> = MaybeUninit::uninit();
        let mut table = unsafe { table.assume_init() };
        for i in 0..array.len() {
            table[i] = array[i].to_ne();
        }
        Self(table)
    }
}
