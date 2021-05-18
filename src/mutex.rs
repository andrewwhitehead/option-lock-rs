use core::{
    fmt::{self, Debug, Formatter},
    ops::{Deref, DerefMut},
};

#[cfg(feature = "alloc")]
use alloc::sync::Arc;

use super::{
    error::{MutexLockError, OptionLockError, PoisonError},
    lock::{OptionGuard, OptionLock},
};

#[cfg(feature = "alloc")]
use super::arc::MutexGuardArc;

/// An `OptionLock` with a guaranteed value.
#[repr(transparent)]
pub struct Mutex<T> {
    pub(crate) inner: OptionLock<T>,
}

impl<T> Mutex<T> {
    /// Create a new mutex instance.
    pub const fn new(value: T) -> Self {
        Self {
            inner: OptionLock::new(value),
        }
    }

    #[inline]
    pub(crate) unsafe fn as_ptr(&self) -> *const T {
        self.inner.as_ptr()
    }

    #[inline]
    pub(crate) unsafe fn as_mut_ptr(&self) -> *mut T {
        self.inner.as_mut_ptr()
    }

    /// Check if a guard is held.
    #[inline]
    pub fn is_locked(&self) -> bool {
        self.inner.is_locked()
    }

    /// Check if the contained value was removed.
    #[inline]
    pub fn is_poisoned(&self) -> bool {
        !self.inner.is_some()
    }

    /// Get a mutable reference to the contained value
    pub fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.as_mut_ptr() }
    }

    /// Unwrap an owned mutex instance.
    pub fn into_inner(self) -> Result<T, PoisonError> {
        self.inner.into_inner().ok_or(PoisonError)
    }

    /// Try to acquire an exclusive lock around the contained value
    #[inline]
    pub fn try_lock(&self) -> Result<MutexGuard<'_, T>, MutexLockError> {
        match self.inner.try_get() {
            Ok(guard) => Ok(guard),
            Err(OptionLockError::FillState) => Err(MutexLockError::Poisoned),
            Err(OptionLockError::Unavailable) => Err(MutexLockError::Unavailable),
        }
    }

    #[cfg(feature = "alloc")]
    /// Try to acquire an exclusive lock for an `Arc<Mutex>`.
    pub fn try_lock_arc(self: &Arc<Self>) -> Result<MutexGuardArc<T>, MutexLockError> {
        self.try_lock()
            .map(|guard| MutexGuardArc::new(self.clone(), guard))
    }

    /// In a spin loop, wait to acquire the mutex.
    pub fn spin_lock(&self) -> Result<MutexGuard<'_, T>, PoisonError> {
        let guard = self.inner.spin_lock();
        if guard.is_none() {
            Err(PoisonError)
        } else {
            Ok(MutexGuard::new(guard))
        }
    }
}

impl<T: Clone> Mutex<T> {
    /// Try to clone the contained resource.
    #[inline]
    pub fn try_clone(&self) -> Result<T, MutexLockError> {
        self.try_lock().map(|g| (*g).clone())
    }
}

impl<T: Copy> Mutex<T> {
    /// Try to copy the contained resource.
    ///
    /// On successful acquisition `Some(T)` is returned. If the lock
    /// is currently held or the value is empty, then `None` is returned.
    #[inline]
    pub fn try_copy(&self) -> Result<T, MutexLockError> {
        self.try_lock().map(|g| *g)
    }
}

impl<T> Debug for Mutex<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Mutex({:?})", &self.inner.state)
    }
}

#[cfg(feature = "std")]
impl<T> ::std::panic::RefUnwindSafe for Mutex<T> {}
#[cfg(feature = "std")]
impl<T> ::std::panic::UnwindSafe for Mutex<T> {}

/// An exclusive guard for a filled [`OptionLock`]
pub struct MutexGuard<'a, T>(OptionGuard<'a, T>);

impl<'a, T> MutexGuard<'a, T> {
    #[inline]
    pub(crate) fn new(guard: OptionGuard<'a, T>) -> Self {
        Self(guard)
    }
}

impl<T> MutexGuard<'_, T> {
    /// Take the value from the mutex. This will result in a `PoisonError` the
    /// next time a lock is attempted.
    pub fn extract(mut slf: Self) -> T {
        slf.0.take().unwrap()
    }

    /// Replace the value in the lock, returning the previous value.
    pub fn replace(slf: &mut Self, value: T) -> T {
        slf.0.replace(value).unwrap()
    }
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().unwrap()
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().unwrap()
    }
}

impl<T: Debug> Debug for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("MutexGuard").field(&**self).finish()
    }
}

#[cfg(feature = "std")]
impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        // Drop the contained value on a panic, because it may not have
        // been left in a consistent state.
        if self.0.is_some() && ::std::thread::panicking() {
            self.0.take();
        }
    }
}

unsafe impl<T: Send> Send for MutexGuard<'_, T> {}
unsafe impl<T: Sync> Sync for MutexGuard<'_, T> {}
