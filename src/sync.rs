use core::ops::DerefMut;
#[cfg(all(feature = "sync", not(feature = "async"), feature = "std"))]
pub(crate) use std::sync::Mutex;

#[cfg(all(feature = "sync", feature = "async", feature = "smol"))]
pub(crate) use smol::sync::Mutex;
#[cfg(all(feature = "sync", not(feature = "std")))]
pub(crate) use spin::Mutex;
#[cfg(all(feature = "sync", feature = "async", feature = "tokio"))]
pub(crate) use tokio::sync::Mutex;

#[cfg(feature = "sync")]
pub struct Shared<T>(alloc::sync::Arc<Mutex<T>>);
#[cfg(not(feature = "sync"))]
pub struct Shared<T>(alloc::rc::Rc<core::cell::RefCell<T>>);

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Shared<T> {
    pub fn new(value: T) -> Self {
        match () {
            #[cfg(feature = "sync")]
            () => Self(alloc::sync::Arc::new(Mutex::new(value))),
            #[cfg(not(feature = "sync"))]
            () => Self(alloc::rc::Rc::new(core::cell::RefCell::new(value))),
        }
    }
}

#[cfg_attr(not(feature = "async"), deasync::deasync)]
impl<T> Shared<T> {
    pub async fn acquire(&self) -> impl DerefMut<Target = T> {
        match () {
            #[cfg(all(feature = "sync", feature = "std", feature = "async"))]
            () => self.0.lock().await,
            #[cfg(all(feature = "sync", feature = "std", not(feature = "async")))]
            () => self.0.lock().unwrap(),
            #[cfg(all(feature = "sync", not(feature = "std")))]
            () => self.0.lock(),
            #[cfg(not(feature = "sync"))]
            () => self.0.borrow_mut(),
        }
    }

    pub async fn try_unwrap(self) -> Result<T, Self> {
        match () {
            #[cfg(all(feature = "sync", any(feature = "tokio", feature = "smol")))]
            () => self.0.try_unwrap().await.map(|mutex| mutex.into_inner()),
            #[cfg(all(feature = "sync", feature = "std", not(feature = "async")))]
            () => self.0.try_unwrap().map(|mutex| mutex.into_inner().unwrap()),
            #[cfg(not(feature = "sync"))]
            () => alloc::rc::Rc::try_unwrap(self.0).map(|cell| cell.into_inner()),
        }
        .map_err(|e| Self(e))
    }
}
