use derive_more::{Display, From, Into};

#[derive(Copy, Clone, Debug, Default, Display, From, Into, Eq, Ord, PartialOrd, PartialEq)]
pub struct SectorID(u64);

impl SectorID {
    pub(crate) const BOOT: Self = Self(0);
}

impl<I: Into<u64>> core::ops::Add<I> for SectorID {
    type Output = Self;

    fn add(self, rhs: I) -> Self {
        Self(self.0 + rhs.into())
    }
}

impl<I: Into<u64>> core::ops::AddAssign<I> for SectorID {
    fn add_assign(&mut self, rhs: I) {
        self.0 += rhs.into()
    }
}

#[derive(Copy, Clone, Debug, Default, Display, From, Into, Eq, Ord, PartialOrd, PartialEq)]
pub struct ClusterID(u32);

impl ClusterID {
    pub(crate) const FIRST: Self = Self(2);

    pub fn valid(&self) -> bool {
        return self.0 > 0;
    }

    pub(crate) fn offset(self) -> u32 {
        self.0 - Self::FIRST.0
    }
}

impl<I: Into<u32>> core::ops::Add<I> for ClusterID {
    type Output = Self;

    fn add(self, rhs: I) -> Self {
        Self(self.0 + rhs.into())
    }
}

impl<I: Into<u32>> core::ops::AddAssign<I> for ClusterID {
    fn add_assign(&mut self, rhs: I) {
        self.0 += rhs.into()
    }
}
