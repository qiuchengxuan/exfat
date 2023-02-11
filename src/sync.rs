#[cfg(all(feature = "sync", not(feature = "async"), feature = "std"))]
pub(crate) use std::sync::Mutex;

#[cfg(all(feature = "sync", feature = "async", feature = "std"))]
pub(crate) use async_std::sync::Mutex;
#[cfg(all(feature = "sync", not(feature = "std")))]
pub(crate) use spin::Mutex;

#[cfg(feature = "sync")]
pub(crate) type Shared<T> = alloc::sync::Arc<Mutex<T>>;
#[cfg(not(feature = "sync"))]
pub(crate) type Shared<T> = alloc::rc::Rc<core::cell::RefCell<T>>;

pub(crate) fn shared<T>(t: T) -> Shared<T> {
    match () {
        #[cfg(feature = "sync")]
        () => alloc::sync::Arc::new(Mutex::new(t)),
        #[cfg(not(feature = "sync"))]
        () => alloc::rc::Rc::new(core::cell::RefCell::new(t)),
    }
}

#[macro_export]
macro_rules! acquire {
    ($shared: expr) => {
        match () {
            #[cfg(all(feature = "sync", feature = "std", feature = "async"))]
            () => $shared.lock().await,
            #[cfg(all(feature = "sync", feature = "std", not(feature = "async")))]
            () => $shared.lock().unwrap(),
            #[cfg(all(feature = "sync", not(feature = "std")))]
            () => $shared.lock(),
            #[cfg(not(feature = "sync"))]
            () => $shared.borrow_mut(),
        }
    };
}

pub(crate) use acquire;
