use core::{
    cell::UnsafeCell,
    fmt::{self, Debug, Formatter},
    hint::spin_loop,
    mem::{self, ManuallyDrop, MaybeUninit},
    ops::{Deref, DerefMut},
    ptr::drop_in_place,
    sync::atomic::{AtomicU8, Ordering},
};

#[cfg(feature = "alloc")]
use alloc::sync::Arc;

#[cfg(feature = "alloc")]
use super::arc::ArcGuard;

pub const SOME: u8 = 0x1;
pub const FREE: u8 = 0x2;
pub const AVAILABLE: u8 = FREE | SOME;

/// A read/write lock around an `Option` value.
pub struct OptionLock<T> {
    data: UnsafeCell<MaybeUninit<T>>,
    pub(crate) state: AtomicU8,
}

impl<T> Default for OptionLock<T> {
    fn default() -> Self {
        Self::empty()
    }
}

unsafe impl<T: Send> Send for OptionLock<T> {}
unsafe impl<T: Send> Sync for OptionLock<T> {}

impl<T> OptionLock<T> {
    /// Create a new, empty instance.
    pub const fn empty() -> Self {
        Self {
            data: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(FREE),
        }
    }

    /// Create a new populated instance.
    pub const fn new(value: T) -> Self {
        Self {
            data: UnsafeCell::new(MaybeUninit::new(value)),
            state: AtomicU8::new(AVAILABLE),
        }
    }

    #[inline]
    pub(crate) unsafe fn as_ptr(&self) -> *const T {
        (&*self.data.get()).as_ptr()
    }

    #[inline]
    pub(crate) unsafe fn as_mut_ptr(&self) -> *mut T {
        (&mut *self.data.get()).as_mut_ptr()
    }

    #[inline]
    pub(crate) fn state(&self) -> u8 {
        self.state.load(Ordering::Relaxed)
    }

    /// Check if there is a stored value and no guard held.
    #[inline]
    pub fn is_some_unlocked(&self) -> bool {
        self.state() == AVAILABLE
    }

    /// Check if there is no stored value and no guard held.
    #[inline]
    pub fn is_none_unlocked(&self) -> bool {
        self.state() == FREE
    }

    /// Check if a guard is held.
    #[inline]
    pub fn is_locked(&self) -> bool {
        self.state() & FREE == 0
    }

    /// Get a mutable reference to the contained value, if any.
    pub fn get_mut(&mut self) -> Option<&mut T> {
        if self.is_some_unlocked() {
            Some(unsafe { &mut *self.as_mut_ptr() })
        } else {
            None
        }
    }

    /// Unwrap an owned lock instance.
    pub fn into_inner(self) -> Option<T> {
        if self.state() & SOME != 0 {
            let slf = ManuallyDrop::new(self);
            Some(unsafe { slf.as_mut_ptr().read() })
        } else {
            None
        }
    }

    /// In a spin loop, wait to acquire the lock.
    pub fn spin_lock(&self) -> OptionGuard<'_, T> {
        loop {
            if let Some(guard) = self.try_lock() {
                break guard;
            }
            // use a relaxed check in spin loop
            while self.state() & FREE == 0 {
                spin_loop();
            }
        }
    }

    /// In a spin loop, wait to acquire the lock with a value of None.
    pub fn spin_lock_none(&self) -> OptionGuard<'_, T> {
        loop {
            if let Some(guard) = self.try_lock_none() {
                break guard;
            }
            // use a relaxed check in spin loop
            while self.state() != FREE {
                spin_loop();
            }
        }
    }

    /// In a spin loop, wait to take a value from the lock.
    pub fn spin_take(&self) -> T {
        loop {
            if let Some(result) = self.try_take() {
                break result;
            }
            // use a relaxed check in spin loop
            while self.state() != AVAILABLE {
                spin_loop();
            }
        }
    }

    /// Try to acquire an exclusive lock.
    ///
    /// On successful acquisition `Some(OptionGuard<'_, T>)` is returned, representing
    /// an exclusive read/write lock.
    pub fn try_lock(&self) -> Option<OptionGuard<'_, T>> {
        let state = self.state.fetch_and(!FREE, Ordering::Release);
        if state & FREE != 0 {
            Some(OptionGuard::new(self, state & SOME != 0))
        } else {
            None
        }
    }

    /// Try to acquire an exclusive lock, but only if the value is currently None.
    ///
    /// On successful acquisition `Some(OptionGuard<'_, T>)` is returned, representing
    /// an exclusive read/write lock.
    pub fn try_lock_none(&self) -> Option<OptionGuard<'_, T>> {
        loop {
            match self
                .state
                .compare_exchange_weak(FREE, 0, Ordering::AcqRel, Ordering::Relaxed)
            {
                Ok(_) => break Some(OptionGuard::new(self, false)),
                Err(FREE) => {
                    // retry
                }
                Err(_) => break None,
            }
        }
    }

    /// Try to acquire an exclusive lock around a contained value.
    ///
    /// On successful acquisition `Some(SomeGuard<T>)` is returned, representing
    /// an exclusive read/write lock.
    pub fn try_get(&self) -> Option<SomeGuard<'_, T>> {
        loop {
            match self.state.compare_exchange_weak(
                AVAILABLE,
                SOME,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => break Some(SomeGuard::new(self)),
                Err(AVAILABLE) => {
                    // retry
                }
                Err(_) => break None,
            }
        }
    }

    #[cfg(feature = "alloc")]
    /// Try to acquire an exclusive lock from a reference to an `Arc<OptionLock>`.
    ///
    /// On successful acquisition `Some(ArcGuard<T>)` is returned, representing
    /// an exclusive read/write lock.
    pub fn try_lock_arc(self: &Arc<Self>) -> Option<ArcGuard<T>> {
        let state = self.state.fetch_and(!FREE, Ordering::Release);
        if state & FREE != 0 {
            Some(ArcGuard::new(self.clone(), state & SOME != 0))
        } else {
            None
        }
    }

    /// Try to take a stored value from the lock.
    ///
    /// On successful acquisition `Some(T)` is returned.
    ///
    /// On failure, `None` is returned. Acquisition can fail either because
    /// there is no contained value, or because the lock is held by a guard.
    #[inline]
    pub fn try_take(&self) -> Option<T> {
        self.try_get().map(SomeGuard::take)
    }

    /// Replace the value in an owned `OptionLock`.
    pub fn replace(&mut self, value: T) -> Option<T> {
        let result = if self.state() & SOME != 0 {
            Some(unsafe { self.as_mut_ptr().read() })
        } else {
            self.state.fetch_or(SOME, Ordering::Relaxed);
            None
        };
        unsafe { self.as_mut_ptr().write(value) };
        result
    }

    /// Take the value (if any) from an owned `OptionLock`.
    pub fn take(&mut self) -> Option<T> {
        if self.state() & SOME != 0 {
            self.state.fetch_and(!SOME, Ordering::Relaxed);
            Some(unsafe { self.as_mut_ptr().read() })
        } else {
            None
        }
    }
}

impl<T: Clone> OptionLock<T> {
    /// Try to clone the contained resource.
    ///
    /// On successful acquisition `Some(T)` is returned. If the lock
    /// is currently held or the value is empty, then `None` is returned.
    #[inline]
    pub fn try_clone(&self) -> Option<T> {
        self.try_get().map(|g| (*g).clone())
    }
}

impl<T: Copy> OptionLock<T> {
    /// Try to copy the contained resource.
    ///
    /// On successful acquisition `Some(T)` is returned. If the lock
    /// is currently held or the value is empty, then `None` is returned.
    #[inline]
    pub fn try_copy(&self) -> Option<T> {
        self.try_get().map(|g| *g)
    }
}

impl<T> Drop for OptionLock<T> {
    fn drop(&mut self) {
        if self.state() & SOME != 0 {
            unsafe {
                drop_in_place(self.as_mut_ptr());
            }
        }
    }
}

impl<T> From<T> for OptionLock<T> {
    fn from(data: T) -> Self {
        Self::new(data)
    }
}

impl<T> From<Option<T>> for OptionLock<T> {
    fn from(data: Option<T>) -> Self {
        if let Some(data) = data {
            Self::new(data)
        } else {
            Self::empty()
        }
    }
}

impl<T> Into<Option<T>> for OptionLock<T> {
    fn into(mut self) -> Option<T> {
        self.take()
    }
}

impl<T> Debug for OptionLock<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OptionLock({})",
            match self.state() {
                FREE => "None",
                AVAILABLE => "Some",
                _ => "Locked",
            }
        )
    }
}

/// An exclusive guard for the value of an [`OptionLock`]
pub struct OptionGuard<'a, T> {
    lock: &'a OptionLock<T>,
    filled: bool,
}

impl<'a, T> OptionGuard<'a, T> {
    #[inline]
    pub(crate) fn new(lock: &'a OptionLock<T>, filled: bool) -> Self {
        Self { lock, filled }
    }
}

impl<T> OptionGuard<'_, T> {
    /// Obtain a shared reference to the contained value, if any.
    pub fn as_ref(&self) -> Option<&T> {
        if self.filled {
            Some(unsafe { &*self.lock.as_ptr() })
        } else {
            None
        }
    }

    /// Obtain an exclusive reference to the contained value, if any.
    pub fn as_mut(&mut self) -> Option<&mut T> {
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
        if self.filled {
            Some(mem::replace(unsafe { &mut *self.lock.as_mut_ptr() }, value))
        } else {
            self.filled = true;
            unsafe {
                self.lock.as_mut_ptr().write(value);
            }
            None
        }
    }

    /// Take the current value from the lock, if any.
    pub fn take(&mut self) -> Option<T> {
        if self.filled {
            self.filled = false;
            Some(unsafe { self.lock.as_ptr().read() })
        } else {
            None
        }
    }
}

impl<T: Debug> Debug for OptionGuard<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("OptionGuard").field(&self.as_ref()).finish()
    }
}

impl<'a, T> Drop for OptionGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.state.store(
            if self.filled { AVAILABLE } else { FREE },
            Ordering::Release,
        );
    }
}

/// An exclusive guard for a filled [`OptionLock`]
pub struct SomeGuard<'a, T> {
    lock: &'a OptionLock<T>,
}

impl<'a, T> SomeGuard<'a, T> {
    #[inline]
    pub(crate) fn new(lock: &'a OptionLock<T>) -> Self {
        Self { lock }
    }
}

impl<T> SomeGuard<'_, T> {
    /// Replace the value in the lock, returning the previous value.
    pub fn replace(&mut self, value: T) -> T {
        mem::replace(unsafe { &mut *self.lock.as_mut_ptr() }, value)
    }

    /// Take the current value from the lock, if any.
    pub fn take(self) -> T {
        let slf = ManuallyDrop::new(self);
        let ret = unsafe { slf.lock.as_ptr().read() };
        slf.lock.state.store(FREE, Ordering::Release);
        ret
    }
}

impl<T> Deref for SomeGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.as_ptr() }
    }
}

impl<T> DerefMut for SomeGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.as_mut_ptr() }
    }
}

impl<T: Debug> Debug for SomeGuard<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SomeGuard").field(&**self).finish()
    }
}

impl<'a, T> Drop for SomeGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.state.store(AVAILABLE, Ordering::Release);
    }
}
