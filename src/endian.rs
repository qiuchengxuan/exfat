use core::fmt::Debug;

#[derive(Copy, Clone, Default, Debug, PartialEq, PartialOrd)]
pub struct Little<T: Copy + Clone + Default + Debug + PartialEq + PartialOrd + Sized>(T);

macro_rules! define {
    ($type:ty) => {
        impl Little<$type> {
            pub fn to_ne(self) -> $type {
                <$type>::from_le(self.0)
            }
        }

        impl Into<$type> for Little<$type> {
            #[inline]
            fn into(self) -> $type {
                <$type>::from_le(self.0)
            }
        }

        impl From<$type> for Little<$type> {
            #[inline]
            fn from(t: $type) -> Self {
                Self(<$type>::to_le(t))
            }
        }
    };
}

define!(u16);
define!(u32);
define!(u64);
define!(usize);
