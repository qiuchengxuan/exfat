use core::ops::DerefMut;
#[cfg(all(feature = "sync", not(feature = "async"), feature = "std"))]
pub(crate) use std::sync::Mutex;

#[cfg(all(feature = "sync", feature = "async", feature = "smol"))]
pub(crate) use smol::lock::Mutex;
#[cfg(all(feature = "sync", not(feature = "std")))]
pub(crate) use spin::Mutex;
#[cfg(all(feature = "sync", feature = "async", feature = "tokio"))]
pub(crate) use tokio::sync::Mutex;

pub trait Share: Sized + Clone {
    type Target;
    #[cfg(feature = "async")]
    fn acquire(&self) -> impl Future<Output = impl DerefMut<Target = Self::Target>>;
    #[cfg(not(feature = "async"))]
    fn acquire(&self) -> impl DerefMut<Target = Self::Target>;
}

#[cfg(all(feature = "tokio"))]
impl<T> Share for std::sync::Arc<tokio::sync::Mutex<T>> {
    type Target = T;
    async fn acquire(&self) -> impl DerefMut<Target = T> {
        self.lock().await
    }
}

#[cfg(all(feature = "smol"))]
impl<T> Share for std::sync::Arc<smol::lock::Mutex<T>> {
    type Target = T;
    async fn acquire(&self) -> impl DerefMut<Target = T> {
        self.lock().await
    }
}

impl<T> Share for alloc::rc::Rc<core::cell::RefCell<T>> {
    type Target = T;
    #[cfg(not(feature = "async"))]
    fn acquire(&self) -> impl DerefMut<Target = T> {
        self.borrow_mut()
    }
    #[cfg(feature = "async")]
    async fn acquire(&self) -> impl DerefMut<Target = T> {
        async { self.borrow_mut() }.await
    }
}

#[cfg(feature = "std")]
impl<T> Share for std::sync::Arc<std::sync::Mutex<T>> {
    type Target = T;
    #[cfg(not(feature = "async"))]
    fn acquire(&self) -> impl DerefMut<Target = T> {
        self.lock().unwrap()
    }
    #[cfg(feature = "async")]
    async fn acquire(&self) -> impl DerefMut<Target = T> {
        async { self.lock().unwrap() }.await
    }
}

impl<T> Share for &'static core::cell::RefCell<T> {
    type Target = T;
    #[cfg(not(feature = "async"))]
    fn acquire(&self) -> impl DerefMut<Target = T> {
        self.borrow_mut()
    }
    #[cfg(feature = "async")]
    async fn acquire(&self) -> impl DerefMut<Target = T> {
        async { self.borrow_mut() }.await
    }
}

#[cfg(feature = "sync")]
pub struct Shared<T>(alloc::sync::Arc<Mutex<T>>);
#[cfg(not(feature = "sync"))]
pub(crate) struct Shared<T>(alloc::rc::Rc<core::cell::RefCell<T>>);

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

#[cfg_attr(not(feature = "async"), maybe_async::must_be_sync)]
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
}
