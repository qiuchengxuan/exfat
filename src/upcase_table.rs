pub(crate) struct UpcaseTable(pub [u16; 128]);

impl UpcaseTable {
    fn lookup(&self, ch: u16) -> u16 {
        match ch > self.0.len() as u16 {
            true => ch,
            false => self.0[ch as usize],
        }
    }

    pub fn equals(&self, left: &str, right: &str) -> bool {
        if left.len() != right.len() {
            return false;
        }
        let (left_chars, right_chars) = (left.chars(), right.chars());
        let chars = left_chars.zip(right_chars);
        for (left_ch, right_ch) in chars {
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
        let mut table = [0u16; 128];
        for i in 0..array.len() {
            table[i] = array[i].to_ne();
        }
        Self(table)
    }
}
