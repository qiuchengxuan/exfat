use crate::types::ClusterID;

#[derive(Copy, Clone, Default)]
pub struct Clusters {
    pub base: ClusterID,
    pub size: u32,
    pub bits: usize,
}

fn lsb(bits: usize) -> usize {
    bits ^ ((bits - 1) & bits)
}

impl Iterator for Clusters {
    type Item = ClusterID;
    fn next(&mut self) -> Option<Self::Item> {
        match (self.bits, self.size) {
            (0, 0) => None,
            (0, _) => {
                self.base += 1u32;
                self.size -= 1;
                Some(self.base - 1u32)
            }
            _ => {
                let bit = lsb(self.bits);
                self.bits ^= bit;
                let shift = bit.ilog2();
                Some(self.base + shift)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::usize;

    use super::*;

    #[test]
    fn test_empty_clusters() {
        let mut clusters = Clusters::default();
        assert_eq!(clusters.next(), None);
        assert_eq!(clusters.next(), None);
    }

    #[test]
    fn test_simple_sequence() {
        let mut clusters = Clusters { base: ClusterID::FIRST, size: 3, bits: 0 };
        assert_eq!(clusters.next(), Some(ClusterID::FIRST));
        assert_eq!(clusters.next(), Some(ClusterID::FIRST + 1u32));
        assert_eq!(clusters.next(), Some(ClusterID::FIRST + 2u32));
        assert_eq!(clusters.next(), None);
        assert_eq!(clusters.next(), None);
    }

    #[test]
    fn test_single_cluster() {
        let mut clusters = Clusters { base: ClusterID::FIRST, size: 1, bits: 0 };
        assert_eq!(clusters.next(), Some(ClusterID::FIRST));
        assert_eq!(clusters.next(), None);
    }

    #[test]
    fn test_bits_with_trailing_zeros() {
        let mut clusters = Clusters { base: ClusterID::FIRST, size: 0, bits: 0b1010 };
        assert_eq!(clusters.next(), Some(ClusterID::FIRST + 1u32));
        assert_eq!(clusters.next(), Some(ClusterID::FIRST + 3u32));
        assert_eq!(clusters.next(), None);
    }

    #[test]
    fn test_bits_all_ones() {
        let mut clusters = Clusters { base: ClusterID::FIRST, size: 0, bits: usize::MAX };
        for i in 0..(core::mem::size_of::<usize>() * 8) {
            assert_eq!(clusters.next(), Some(ClusterID::FIRST + i as u32));
        }
        assert_eq!(clusters.next(), None);
    }
}
