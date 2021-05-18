use alloc::sync::Arc;
use core::{
    fmt::{self, Debug, Formatter},
    mem,
    ops::{Deref, DerefMut},
};

use super::lock::{OptionGuard, OptionLock};
use super::mutex::{Mutex, MutexGuard};

/// A write guard for the value of an [`Arc<OptionLock>`]
pub struct OptionGuardArc<T> {
    lock: Arc<OptionLock<T>>,
    filled: bool,
}

impl<T> OptionGuardArc<T> {
    #[inline]
    pub(crate) fn new(lock: Arc<OptionLock<T>>, guard: OptionGuard<'_, T>) -> Self {
        let result = Self {
            lock,
            filled: guard.is_some(),
        };
        mem::forget(guard);
        result
    }

    /// Obtain a shared reference to the contained value, if any.
    pub fn as_ref(&self) -> Option<&T> {
        if self.filled {
            Some(unsafe { &*self.lock.as_mut_ptr() })
        } else {
            None
        }
    }

    /// Obtain an exclusive reference to the contained value, if any.
    pub fn as_mut_ref(&mut self) -> Option<&mut T> {
        if self.filled {
            Some(unsafe { &mut *self.lock.as_mut_ptr() })
        } else {
            None
        }
    }

    /// Check if the lock contains `None`.
    #[inline]
    pub fn is_none(&self) -> bool {
        !self.filled
    }

    /// Check if the lock contains `Some(T)`.
    #[inline]
    pub fn is_some(&self) -> bool {
        self.filled
    }

    /// Replace the value in the lock, returning the previous value, if any.
    pub fn replace(&mut self, value: T) -> Option<T> {
        let ret = if self.filled {
            Some(unsafe { self.lock.as_mut_ptr().read() })
        } else {
            self.filled = true;
            None
        };
        unsafe {
            self.lock.as_mut_ptr().write(value);
        }
        ret
    }

    /// Take the current value from the lock, if any.
    pub fn take(&mut self) -> Option<T> {
        if self.filled {
            self.filled = false;
            Some(unsafe { self.lock.as_mut_ptr().read() })
        } else {
            None
        }
    }
}

impl<T: Debug> Debug for OptionGuardArc<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("OptionGuardArc")
            .field(&self.as_ref())
            .finish()
    }
}

impl<T> Drop for OptionGuardArc<T> {
    fn drop(&mut self) {
        let _ = OptionGuard::new(&self.lock, self.filled);
    }
}

unsafe impl<T: Send> Send for OptionGuardArc<T> {}
unsafe impl<T: Sync> Sync for OptionGuardArc<T> {}

/// A write guard for an [`Arc<Mutex>`]
pub struct MutexGuardArc<T> {
    lock: Arc<Mutex<T>>,
}

impl<T> MutexGuardArc<T> {
    #[inline]
    pub(crate) fn new(lock: Arc<Mutex<T>>, guard: MutexGuard<'_, T>) -> Self {
        let result = Self { lock };
        mem::forget(guard);
        result
    }

    /// Replace the value in the lock, returning the previous value.
    pub fn replace(&mut self, value: T) -> T {
        mem::replace(unsafe { &mut *self.lock.as_mut_ptr() }, value)
    }
}

impl<T> Deref for MutexGuardArc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.as_ptr() }
    }
}

impl<T> DerefMut for MutexGuardArc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.as_mut_ptr() }
    }
}

impl<T: Debug> Debug for MutexGuardArc<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("MutexGuardArc").field(&**self).finish()
    }
}

impl<T> Drop for MutexGuardArc<T> {
    fn drop(&mut self) {
        let _ = OptionGuard::new(&self.lock.inner, true);
    }
}

unsafe impl<T: Send> Send for MutexGuardArc<T> {}
unsafe impl<T: Sync> Sync for MutexGuardArc<T> {}
