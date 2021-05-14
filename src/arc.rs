use core::fmt::{self, Debug, Formatter};
use core::sync::atomic::Ordering;

use alloc::sync::Arc;

use super::lock::{OptionLock, AVAILABLE, FREE};

/// A write guard for the value of an [`Arc<OptionLock>`]
pub struct ArcGuard<T> {
    lock: Arc<OptionLock<T>>,
    filled: bool,
}

impl<T> ArcGuard<T> {
    #[inline]
    pub(crate) fn new(lock: Arc<OptionLock<T>>, filled: bool) -> Self {
        Self { lock, filled }
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

impl<T: Debug> Debug for ArcGuard<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ArcGuard").field(&self.as_ref()).finish()
    }
}

impl<T> Drop for ArcGuard<T> {
    fn drop(&mut self) {
        self.lock.state.store(
            if self.filled { AVAILABLE } else { FREE },
            Ordering::Release,
        );
    }
}
