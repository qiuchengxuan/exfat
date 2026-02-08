use derive_more::{Display, From, Into};

pub type Sector = u64;

#[derive(Copy, Clone, Debug, Default, Display, From, Into, Eq, Ord, PartialOrd, PartialEq)]
pub struct ClusterID(u32);

impl ClusterID {
    pub(crate) const FIRST: Self = Self(2);

    pub fn valid(&self) -> bool {
        return self.0 > 0;
    }

    pub(crate) fn offset(self) -> u32 {
        (self.0 - Self::FIRST.0) / 8
    }

    pub(crate) fn bit(self) -> u8 {
        (self.0 - Self::FIRST.0) as u8 % 8
    }
}

impl<I: Into<u32>> core::ops::Add<I> for ClusterID {
    type Output = Self;

    fn add(self, rhs: I) -> Self {
        Self(self.0 + rhs.into())
    }
}

impl<I: Into<u32>> core::ops::Sub<I> for ClusterID {
    type Output = Self;

    fn sub(self, rhs: I) -> Self {
        Self(self.0 - rhs.into())
    }
}

impl<I: Into<u32>> core::ops::AddAssign<I> for ClusterID {
    fn add_assign(&mut self, rhs: I) {
        self.0 += rhs.into()
    }
}
