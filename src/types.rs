use crate::{endian::Little, error::Error};
use derive_more::{Display, From, Into};

pub type Result<T, E> = core::result::Result<T, Error<E>>;

pub type Sector = u64;

#[derive(Copy, Clone, Debug, Default, Display, From, Into, Eq, Ord, PartialOrd, PartialEq)]
pub struct Cluster(u32);

impl Into<Little<u32>> for Cluster {
    fn into(self) -> Little<u32> {
        Little::from(self.0)
    }
}

impl From<Little<u32>> for Cluster {
    fn from(value: Little<u32>) -> Self {
        Self(value.to_ne())
    }
}

impl Cluster {
    pub(crate) const FIRST: Self = Self(2);

    pub fn valid(&self) -> bool {
        return self.0 > 0;
    }

    pub(crate) fn index(self) -> u32 {
        self.0 - Self::FIRST.0
    }
}

impl<I: Into<u32>> core::ops::Add<I> for Cluster {
    type Output = Self;

    fn add(self, rhs: I) -> Self {
        Self(self.0 + rhs.into())
    }
}

impl<I: Into<u32>> core::ops::Sub<I> for Cluster {
    type Output = Self;

    fn sub(self, rhs: I) -> Self {
        Self(self.0 - rhs.into())
    }
}

impl<I: Into<u32>> core::ops::AddAssign<I> for Cluster {
    fn add_assign(&mut self, rhs: I) {
        self.0 += rhs.into()
    }
}
