use core::{
    cell::Cell,
    fmt::{self, Debug, Display, Formatter},
    hint::spin_loop,
    ops::Deref,
};

use super::lock::OptionLock;

/// An `Option` value which can be safely written once.
pub struct OnceCell<T>(OptionLock<T>);

impl<T> OnceCell<T> {
    /// Create a new, empty `OnceCell`.
    pub const fn empty() -> Self {
        Self(OptionLock::empty())
    }

    /// Create a `OnceCell` from an owned value.
    pub const fn new(value: T) -> Self {
        Self(OptionLock::new(value))
    }

    /// Get a shared reference to the contained value, if any.
    pub fn get(&self) -> Option<&T> {
        if self.0.is_some_unlocked() {
            // safe because the value is never reassigned
            Some(unsafe { &*self.0.as_ptr() })
        } else {
            None
        }
    }

    /// Get a mutable reference to the contained value, if any.
    pub fn get_mut(&mut self) -> Option<&mut T> {
        self.0.get_mut()
    }

    /// Get a reference to the contained value, initializing it if necessary.
    /// The initializer will only be run by one thread if multiple are in competition.
    pub fn get_or_init(&self, init: impl FnOnce() -> T) -> &T {
        loop {
            if self.0.is_some_unlocked() {
                return unsafe { &*self.0.as_ptr() };
            }
            if let Some(mut guard) = self.0.try_lock_none() {
                guard.replace(init());
                return unsafe { &*self.0.as_ptr() };
            } else {
                while self.0.is_locked() {
                    spin_loop();
                }
            }
        }
    }

    /// Get a reference to the contained value, initializing it if necessary.
    /// The initializer will only be run by one thread if multiple are in competition.
    pub fn get_or_try_init<E>(&self, init: impl FnOnce() -> Result<T, E>) -> Result<&T, E> {
        loop {
            if self.0.is_some_unlocked() {
                return Ok(unsafe { &*self.0.as_ptr() });
            }
            if let Some(mut guard) = self.0.try_lock_none() {
                guard.replace(init()?);
                return Ok(unsafe { &*self.0.as_ptr() });
            } else {
                while self.0.is_locked() {
                    spin_loop();
                }
            }
        }
    }

    /// Assign the value of the OnceCell, returning `Some(value)` if
    /// the cell is already locked or populated.
    pub fn set(&self, value: T) -> Result<(), T> {
        if let Some(mut guard) = self.0.try_lock_none() {
            guard.replace(value);
            Ok(())
        } else {
            Err(value)
        }
    }

    /// Extract the inner value.
    pub fn into_inner(self) -> Option<T> {
        self.0.into_inner()
    }

    /// Check if the lock is currently acquired.
    pub fn is_locked(&self) -> bool {
        self.0.is_locked()
    }
}

impl<T: Clone> Clone for OnceCell<T> {
    fn clone(&self) -> Self {
        Self::from(self.get().cloned())
    }
}

impl<T> Default for OnceCell<T> {
    fn default() -> Self {
        Self(None.into())
    }
}

impl<T: Debug> Debug for OnceCell<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("OnceCell").field(&self.get()).finish()
    }
}

impl<T: Display> Display for OnceCell<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(val) = self.get() {
            Display::fmt(val, f)
        } else {
            write!(f, "None")
        }
    }
}

impl<T> From<T> for OnceCell<T> {
    fn from(data: T) -> Self {
        Self(data.into())
    }
}

impl<T> From<Option<T>> for OnceCell<T> {
    fn from(data: Option<T>) -> Self {
        Self(data.into())
    }
}

impl<T> From<OptionLock<T>> for OnceCell<T> {
    fn from(lock: OptionLock<T>) -> Self {
        Self(lock)
    }
}

/// A convenient wrapper around a `OnceCell<T>` with an initializer.
pub struct Lazy<T, F = fn() -> T> {
    cell: OnceCell<T>,
    init: Cell<Option<F>>,
}

unsafe impl<T, F: Send> Sync for Lazy<T, F> where OnceCell<T>: Sync {}

impl<T, F> Lazy<T, F> {
    /// Create a new Lazy instance
    pub const fn new(init: F) -> Self {
        Self {
            cell: OnceCell::empty(),
            init: Cell::new(Some(init)),
        }
    }
}

impl<T: Debug, F> Debug for Lazy<T, F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lazy")
            .field("cell", &self.cell)
            .field("init", &"..")
            .finish()
    }
}

impl<T: Display, F: FnOnce() -> T> Display for Lazy<T, F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&**self, f)
    }
}

impl<T, F: FnOnce() -> T> Lazy<T, F> {
    /// Ensure that the initializer has run
    pub fn force(this: &Self) -> &T {
        this.cell.get_or_init(|| (this.init.take().unwrap())())
    }
}

impl<T: Default> Default for Lazy<T> {
    fn default() -> Lazy<T> {
        Lazy::new(T::default)
    }
}

impl<T, F: FnOnce() -> T> Deref for Lazy<T, F> {
    type Target = T;

    fn deref(&self) -> &T {
        Lazy::force(self)
    }
}
