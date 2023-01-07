use derive_more::{From, Into};

#[derive(Copy, Clone, Debug, Default, From, Into)]
pub struct SectorID(u64);

impl<I: Into<u64>> core::ops::Add<I> for SectorID {
    type Output = Self;

    fn add(self, rhs: I) -> Self {
        Self(self.0 + rhs.into())
    }
}

#[derive(Copy, Clone, Debug, Default, From, Into)]
pub struct ClusterID(u32);

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
