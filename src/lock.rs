use core::{
    cell::UnsafeCell,
    fmt::{self, Debug, Formatter},
    hint::spin_loop,
    mem::{self, transmute, ManuallyDrop, MaybeUninit},
    ops::Deref,
    ptr::drop_in_place,
    sync::atomic::{AtomicU8, Ordering},
};

#[cfg(feature = "alloc")]
use alloc::sync::Arc;

#[cfg(feature = "alloc")]
use super::arc::{MutexGuardArc, OptionGuardArc};

use super::error::OptionLockError;

use super::mutex::MutexGuard;

#[repr(transparent)]
pub(crate) struct State(AtomicU8);

impl State {
    pub const FREE: u8 = 1 << 0;
    pub const SOME: u8 = 1 << 1;
    pub const AVAILABLE: u8 = Self::FREE | Self::SOME;

    pub const fn new(value: u8) -> Self {
        Self(AtomicU8::new(value))
    }

    #[inline]
    pub fn is_some_mut(&mut self) -> bool {
        *self.0.get_mut() & State::SOME != 0
    }

    #[inline]
    pub fn value(&self) -> u8 {
        self.0.load(Ordering::Relaxed)
    }
}

impl Deref for State {
    type Target = AtomicU8;

    #[inline]
    fn deref(&self) -> &AtomicU8 {
        &self.0
    }
}

impl Debug for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0.load(Ordering::Relaxed) {
            Self::FREE => "None",
            Self::AVAILABLE => "Some",
            _ => "Locked",
        })
    }
}

impl PartialEq<u8> for State {
    #[inline]
    fn eq(&self, other: &u8) -> bool {
        self.0.load(Ordering::Relaxed) == *other
    }
}

/// A read/write lock around an `Option` value.
pub struct OptionLock<T> {
    data: UnsafeCell<MaybeUninit<T>>,
    pub(crate) state: State,
}

impl<T> Default for OptionLock<T> {
    fn default() -> Self {
        Self::empty()
    }
}

unsafe impl<T: Send> Send for OptionLock<T> {}
unsafe impl<T: Send> Sync for OptionLock<T> {}

impl<T> OptionLock<T> {
    /// Create a new instance with no stored value.
    pub const fn empty() -> Self {
        Self {
            data: UnsafeCell::new(MaybeUninit::uninit()),
            state: State::new(State::FREE),
        }
    }

    /// Create a new populated instance.
    pub const fn new(value: T) -> Self {
        Self {
            data: UnsafeCell::new(MaybeUninit::new(value)),
            state: State::new(State::AVAILABLE),
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

    /// Check if there is no stored value and no guard held.
    #[inline]
    pub fn is_none_unlocked(&self) -> bool {
        self.state == State::FREE
    }

    /// Check if there is a stored value and no guard held.
    #[inline]
    pub fn is_some_unlocked(&self) -> bool {
        self.state == State::AVAILABLE
    }

    #[inline]
    pub(crate) fn is_some(&self) -> bool {
        self.state.value() & State::SOME != 0
    }

    /// Check if a guard is held.
    #[inline]
    pub fn is_locked(&self) -> bool {
        self.state.value() & State::FREE == 0
    }

    /// Get a mutable reference to the contained value, if any.
    pub fn get_mut(&mut self) -> Option<&mut T> {
        if self.is_some() {
            Some(unsafe { &mut *self.as_mut_ptr() })
        } else {
            None
        }
    }

    /// Unwrap an owned lock instance.
    pub fn into_inner(mut self) -> Option<T> {
        if self.state.is_some_mut() {
            let slf = ManuallyDrop::new(self);
            Some(unsafe { slf.as_mut_ptr().read() })
        } else {
            None
        }
    }

    /// In a spin loop, wait to get an exclusive lock on the contained value.
    pub fn spin_get(&self) -> MutexGuard<'_, T> {
        loop {
            if let Ok(guard) = self.try_get() {
                return guard;
            }
            while !self.is_some_unlocked() {
                spin_loop();
            }
        }
    }

    /// In a spin loop, wait to acquire the lock.
    pub fn spin_lock(&self) -> OptionGuard<'_, T> {
        loop {
            if let Ok(guard) = self.try_lock() {
                return guard;
            }
            while self.is_locked() {
                spin_loop();
            }
        }
    }

    /// In a spin loop, wait to acquire the lock with an empty slot.
    pub fn spin_lock_none(&self) -> OptionGuard<'_, T> {
        loop {
            if let Ok(guard) = self.try_lock_none() {
                return guard;
            }
            while !self.is_none_unlocked() {
                spin_loop();
            }
        }
    }

    /// In a spin loop, wait to take a value from the lock.
    pub fn spin_take(&self) -> T {
        loop {
            if let Ok(result) = self.try_take() {
                return result;
            }
            while !self.is_some_unlocked() {
                spin_loop();
            }
        }
    }

    /// Try to acquire an exclusive lock around a contained value.
    ///
    /// On successful acquisition a `MutexGuard<'_, T>` is returned, representing
    /// an exclusive read/write lock.
    pub fn try_get(&self) -> Result<MutexGuard<'_, T>, OptionLockError> {
        match self.state.compare_exchange(
            State::AVAILABLE,
            State::SOME,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => Ok(MutexGuard::new(OptionGuard::new(self, true))),
            Err(State::FREE) => Err(OptionLockError::FillState),
            Err(_) => Err(OptionLockError::Unavailable),
        }
    }

    #[cfg(feature = "alloc")]
    /// Try to acquire an exclusive lock around the value in an `Arc<OptionLock>`.
    ///
    /// On successful acquisition a `MutexGuardArc<T>` is returned, representing
    /// an exclusive read/write lock.
    pub fn try_get_arc(self: &Arc<Self>) -> Result<MutexGuardArc<T>, OptionLockError> {
        self.try_get()
            .map(|guard| MutexGuardArc::new(unsafe { transmute(self.clone()) }, guard))
    }

    /// Try to store a value, if the slot is currently empty and a lock can be acquired.
    pub fn try_fill(&self, value: T) -> Result<(), T> {
        match self
            .state
            .compare_exchange(State::FREE, 0, Ordering::AcqRel, Ordering::Relaxed)
        {
            Ok(_) => {
                OptionGuard::new(self, false).replace(value);
                Ok(())
            }
            Err(_) => Err(value),
        }
    }

    /// Store the result of an initializer function if the slot is currently empty
    /// and a lock can be acquired. If a lock cannot be acquired, then the initializer
    /// is never called.
    pub fn try_fill_with(&self, f: impl FnOnce() -> T) -> Result<(), OptionLockError> {
        self.try_lock_none()?.replace(f());
        Ok(())
    }

    /// Try to acquire an exclusive lock.
    ///
    /// On successful acquisition an `OptionGuard<'_, T>` is returned, representing
    /// an exclusive read/write lock.
    pub fn try_lock(&self) -> Result<OptionGuard<'_, T>, OptionLockError> {
        let state = self.state.fetch_and(!State::FREE, Ordering::Release);
        if state & State::FREE != 0 {
            Ok(OptionGuard::new(self, state & State::SOME != 0))
        } else {
            Err(OptionLockError::Unavailable)
        }
    }

    #[cfg(feature = "alloc")]
    /// Try to acquire an exclusive lock from a reference to an `Arc<OptionLock>`.
    ///
    /// On successful acquisition an `OptionGuardArc<T>` is returned, representing
    /// an exclusive read/write lock.
    pub fn try_lock_arc(self: &Arc<Self>) -> Result<OptionGuardArc<T>, OptionLockError> {
        self.try_lock()
            .map(|guard| OptionGuardArc::new(self.clone(), guard))
    }

    /// Try to acquire an exclusive lock when there is no value currently stored.
    pub fn try_lock_none(&self) -> Result<OptionGuard<'_, T>, OptionLockError> {
        match self
            .state
            .compare_exchange(State::FREE, 0, Ordering::AcqRel, Ordering::Relaxed)
        {
            Ok(_) => return Ok(OptionGuard::new(self, false)),
            Err(State::AVAILABLE) => Err(OptionLockError::FillState),
            Err(_) => Err(OptionLockError::Unavailable),
        }
    }

    #[cfg(feature = "alloc")]
    /// Try to acquire an exclusive lock when there is no value currently stored.
    pub fn try_lock_empty_arc(self: &Arc<Self>) -> Result<OptionGuardArc<T>, OptionLockError> {
        self.try_lock_none()
            .map(|guard| OptionGuardArc::new(self.clone(), guard))
    }

    /// Try to take a stored value from the lock.
    #[inline]
    pub fn try_take(&self) -> Result<T, OptionLockError> {
        self.try_get().map(MutexGuard::extract)
    }

    /// Replace the value in an owned `OptionLock`.
    pub fn replace(&mut self, value: T) -> Option<T> {
        let result = if self.is_some() {
            Some(unsafe { self.as_mut_ptr().read() })
        } else {
            self.state.fetch_or(State::SOME, Ordering::Relaxed);
            None
        };
        unsafe { self.as_mut_ptr().write(value) };
        result
    }

    /// Take the value (if any) from an owned `OptionLock`.
    pub fn take(&mut self) -> Option<T> {
        if self.is_some() {
            self.state.fetch_and(!State::SOME, Ordering::Relaxed);
            Some(unsafe { self.as_mut_ptr().read() })
        } else {
            None
        }
    }
}

impl<T: Clone> OptionLock<T> {
    /// Try to clone the contained resource.
    #[inline]
    pub fn try_clone(&self) -> Result<T, OptionLockError> {
        self.try_get().map(|g| (*g).clone())
    }
}

impl<T: Copy> OptionLock<T> {
    /// Try to copy the contained resource.
    #[inline]
    pub fn try_copy(&self) -> Result<T, OptionLockError> {
        self.try_get().map(|g| *g)
    }
}

impl<T> Drop for OptionLock<T> {
    fn drop(&mut self) {
        if self.state.is_some_mut() {
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
        write!(f, "OptionLock({:?})", &self.state)
    }
}

/// An exclusive guard for the value of an [`OptionLock`]
pub struct OptionGuard<'a, T> {
    lock: &'a OptionLock<T>,
    is_some: bool,
}

impl<'a, T> OptionGuard<'a, T> {
    #[inline]
    pub(crate) fn new(lock: &'a OptionLock<T>, is_some: bool) -> Self {
        Self { lock, is_some }
    }
}

impl<T> OptionGuard<'_, T> {
    /// Obtain a shared reference to the contained value, if any.
    pub fn as_ref(&self) -> Option<&T> {
        if self.is_some {
            Some(unsafe { &*self.lock.as_ptr() })
        } else {
            None
        }
    }

    /// Obtain an exclusive reference to the contained value, if any.
    pub fn as_mut(&mut self) -> Option<&mut T> {
        if self.is_some {
            Some(unsafe { &mut *self.lock.as_mut_ptr() })
        } else {
            None
        }
    }

    /// Check if the lock contains `None`.
    #[inline]
    pub fn is_none(&self) -> bool {
        !self.is_some
    }

    /// Check if the lock contains `Some(T)`.
    #[inline]
    pub fn is_some(&self) -> bool {
        self.is_some
    }

    /// Replace the value in the lock, returning the previous value, if any.
    pub fn replace(&mut self, value: T) -> Option<T> {
        if self.is_some {
            Some(mem::replace(unsafe { &mut *self.lock.as_mut_ptr() }, value))
        } else {
            self.is_some = true;
            unsafe {
                self.lock.as_mut_ptr().write(value);
            }
            None
        }
    }

    /// Take the current value from the lock, if any.
    pub fn take(&mut self) -> Option<T> {
        if self.is_some {
            self.is_some = false;
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
            if self.is_some {
                State::AVAILABLE
            } else {
                State::FREE
            },
            Ordering::Release,
        );
    }
}

unsafe impl<T: Send> Send for OptionGuard<'_, T> {}
unsafe impl<T: Sync> Sync for OptionGuard<'_, T> {}
