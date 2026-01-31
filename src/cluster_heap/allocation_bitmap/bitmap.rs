use core::cmp::min;
use core::ops::Range;
use core::slice::from_raw_parts;
use core::{marker::PhantomData, ops::Index};

const USIZE_BITS: usize = usize::BITS as usize;

pub struct LittleEndian;
pub trait ByteOrder {}
impl ByteOrder for LittleEndian {}

const unsafe fn bytes_to_usizes(blocks: &[u8]) -> &[usize] {
    let size = blocks.len() / core::mem::size_of::<usize>();
    unsafe { from_raw_parts::<usize>(blocks.as_ptr().cast(), size) }
}

pub struct BitmapBitsIter<'a, E, const N: usize> {
    raw: &'a [[u8; N]],
    offset: usize,
    limit: usize,
    phantom: PhantomData<E>,
}

pub trait BitmapIterator {
    type Item;

    fn next(&mut self) -> Option<Self::Item>;
    fn nth(&mut self, n: usize) -> Option<Self::Item>;

    fn skip(mut self, n: usize) -> Self
    where
        Self: Sized,
    {
        self.nth(n);
        self
    }

    fn take(self, n: usize) -> Self
    where
        Self: Sized;

    fn position<P>(&mut self, predicate: P) -> Option<usize>
    where
        P: FnMut(Self::Item) -> bool;

    fn any<P>(&mut self, predicate: P) -> bool
    where
        P: FnMut(Self::Item) -> bool,
    {
        self.position(predicate).is_some()
    }
}

impl<'a, const N: usize> BitmapIterator for BitmapBitsIter<'a, LittleEndian, N> {
    type Item = bool;

    fn next(&mut self) -> Option<bool> {
        if self.offset >= self.limit {
            return None;
        }
        let offset = self.offset / 8;
        self.offset += 1;
        Some((u8::from_le(self.raw[offset / N][offset % N]) >> (offset % 8)) & 1 > 0)
    }

    fn nth(&mut self, n: usize) -> Option<bool> {
        if n > 0 {
            self.offset = core::cmp::min(self.offset.saturating_add(n - 1), self.limit);
        }
        self.next()
    }

    fn take(mut self, n: usize) -> Self {
        self.limit = core::cmp::min(self.offset.saturating_add(n), self.limit);
        self
    }

    fn position<P>(&mut self, mut predicate: P) -> Option<usize>
    where
        P: FnMut(Self::Item) -> bool,
    {
        if self.offset >= self.limit {
            return None;
        }
        let expect = if predicate(true) { 0 } else { usize::MAX };
        let segments = unsafe { bytes_to_usizes(self.raw.as_flattened()) };
        let mut start = self.offset / USIZE_BITS;
        let shift = self.offset % USIZE_BITS;
        if shift > 0 {
            let mut first = (usize::from_le(segments[start]) >> shift) ^ expect;
            if self.limit / USIZE_BITS == self.offset / USIZE_BITS {
                first &= (1 << (self.limit - self.offset)) - 1
            }
            if first > 0 {
                let offset = (first & first.wrapping_neg()).ilog2() as usize;
                self.offset = start * USIZE_BITS + offset + 1;
                return Some(offset);
            } else if self.limit / USIZE_BITS == self.offset / USIZE_BITS {
                return None;
            }
            start += 1;
        }
        let end = (self.limit + USIZE_BITS - 1) / USIZE_BITS;

        let Some(position) = segments[start..end].iter().copied().position(|v| v != expect) else {
            self.offset = self.limit;
            return None;
        };
        let index = start + position;
        let value = usize::from_le(segments[index]) ^ expect;
        let offset = index * USIZE_BITS + (value & value.wrapping_neg()).ilog2() as usize;
        let count = offset - self.offset;
        self.offset = core::cmp::min(offset + 1, self.limit);
        if offset < self.limit { Some(count) } else { None }
    }
}

pub struct BitmapBits<'a, E, const N: usize> {
    raw: &'a [[u8; N]],
    phantom: PhantomData<E>,
}

impl<'a, E, const N: usize> Index<usize> for BitmapBits<'a, E, N> {
    type Output = bool;
    fn index(&self, index: usize) -> &Self::Output {
        let offset = index / 8;
        let bit = (self.raw[offset / N][offset % N] >> (index % 8)) & 1;
        if bit > 0 { &true } else { &false }
    }
}

pub struct Masking {
    offset: usize,
    remain: usize,
    first: Option<usize>,
    bit: bool,
    last: usize,
}

impl Iterator for Masking {
    type Item = (usize, usize);
    fn next(&mut self) -> Option<Self::Item> {
        if self.remain == 0 {
            return None;
        }
        let offset = self.offset;
        self.offset += size_of::<usize>();
        self.remain -= 1;
        let mid_value = if self.bit { usize::MAX } else { 0 };
        let value = match (self.first.take(), self.remain) {
            (Some(value), _) => value,
            (None, 0) => self.last,
            _ => mid_value,
        };
        return Some((offset, value));
    }
}

impl<'a, const N: usize> BitmapBits<'a, LittleEndian, N> {
    pub fn iter(&self) -> BitmapBitsIter<'a, LittleEndian, N> {
        let limit = self.raw.len() * N * 8;
        BitmapBitsIter { raw: self.raw, offset: 0, limit, phantom: PhantomData }
    }

    pub fn masking(&self, range: Range<usize>, bit: bool) -> Masking {
        let segments = unsafe { bytes_to_usizes(self.raw.as_flattened()) };
        let Range { start, mut end } = range;
        end = min(end, self.raw.len() * N * 8);
        let length = (end + USIZE_BITS - 1) / USIZE_BITS - start / USIZE_BITS;

        let mut first = None;
        let offset = start % USIZE_BITS;
        let (start_index, end_index) = (start / USIZE_BITS, end / USIZE_BITS);
        if offset > 0 || length == 1 {
            let segment = usize::from_le(segments[start_index]);
            let mut bits = usize::MAX << offset;
            if start_index == end_index {
                bits &= (1 << (end % USIZE_BITS)) - 1
            }
            first = Some(if bit { segment | bits } else { segment & !bits });
        }

        let mut last = 0;
        if length > 1 {
            let offset = end % USIZE_BITS;
            if offset > 0 {
                let segment = usize::from_le(segments[end / USIZE_BITS]);
                let bits = usize::MAX >> (USIZE_BITS - offset);
                last = if bit { segment | bits } else { segment & !bits };
            }
        }

        Masking { offset: start / size_of::<usize>(), remain: length, first, bit, last }
    }
}

pub struct Bitmap<'a, E, const N: usize> {
    raw: &'a [[u8; N]],
    phantom: PhantomData<E>,
}

impl<'a, E: ByteOrder, const N: usize> Bitmap<'a, E, N> {
    const _ALIGN_CHECK: () = assert!(N % core::mem::size_of::<usize>() == 0);

    pub fn new(raw: &'a [[u8; N]], _order: E) -> Self {
        Self { raw, phantom: PhantomData }
    }

    pub const fn bits(&self) -> BitmapBits<'a, E, N> {
        BitmapBits { raw: self.raw, phantom: PhantomData }
    }
}

impl<'a, E, const N: usize> Index<usize> for Bitmap<'a, E, N> {
    type Output = u8;
    fn index(&self, index: usize) -> &Self::Output {
        &self.raw[index / N][index % N]
    }
}

#[cfg(test)]
mod test {
    use core::mem::size_of;
    use std::usize;

    use super::{Bitmap, BitmapIterator, LittleEndian};

    #[test]
    fn test_bitmap() {
        let mut raw = [[0u8; 512]; 1];
        raw[0][1] = 0x7;
        raw[0][100] = 0x1;
        let bitmap = Bitmap::new(raw.as_slice(), LittleEndian);
        assert_eq!(bitmap[1], 0x7);
        assert_eq!(bitmap.bits()[7], false);
        assert_eq!(bitmap.bits()[8], true);
        assert_eq!(bitmap.bits()[10], true);
        assert_eq!(bitmap.bits()[11], false);
        assert_eq!(bitmap.bits().iter().position(|v| v), Some(8));
        assert_eq!(bitmap.bits().iter().skip(4).position(|v| v), Some(4));
        assert_eq!(bitmap.bits().iter().skip(4).position(|v| !v), Some(0));
        assert_eq!(bitmap.bits().iter().skip(4).take(3).position(|v| v), None);
        assert_eq!(bitmap.bits().iter().take(4).position(|v| v), None);
        assert_eq!(bitmap.bits().iter().skip(799).position(|v| v), Some(1));
        assert_eq!(bitmap.bits().iter().skip(800).position(|v| v), Some(0));
        assert_eq!(bitmap.bits().iter().skip(801).position(|v| v), None);
        assert_eq!(bitmap.bits().iter().skip(801).take(usize::MAX).position(|v| v), None);
        assert_eq!(bitmap.bits().iter().skip(usize::MAX).position(|v| v), None);
    }

    #[test]
    fn test_masking() {
        let raw = [[0u8; 512]; 1];
        let bitmap = Bitmap::new(raw.as_slice(), LittleEndian);
        let entries: Vec<_> = bitmap.bits().masking(0..1, true).collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, 0);
        assert_eq!(entries[0].1, 1);

        let entries: Vec<_> = bitmap.bits().masking(1..65, true).collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, 0);
        assert_eq!(entries[1].0, size_of::<usize>());

        let entries: Vec<_> = bitmap.bits().masking(1..129, true).collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0, 0);
        assert_eq!(entries[1].0, size_of::<usize>());
        assert_eq!(entries[1].1, usize::MAX);
        assert_eq!(entries[2].0, size_of::<usize>() * 2);
    }
}
