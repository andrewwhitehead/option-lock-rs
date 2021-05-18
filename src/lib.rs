//! This crate defines a locking structure wrapping an `Option` value. The lock
//! can be acquired using a single atomic operation. The `OptionLock` structure
//! is represented as one atomic `u8` variable along with the current value of
//! the lock (which may be empty). It can be constructed in `const` contexts.
//!
//! The `try_lock` and `try_take` operations are non-blocking and appropriate
//! for using within a polled `Future`, but the lock cannot register wakers or
//! automatically park the current thread. A traditional `Mutex` or the
//! `async-lock` crate may be used in this case.
//!
//! This structure allows for multiple usage patterns. A basic example (in this
//! case an AtomicI32 could be substituted):
//!
//! ```
//! use option_lock::{OptionLock, OptionGuard};
//!
//! static SHARED: OptionLock<i32> = OptionLock::new(0);
//!
//! fn try_increase() -> bool {
//!   if let Ok(mut guard) = SHARED.try_lock() {
//!     let next = guard.take().unwrap() + 1;
//!     OptionGuard::replace(&mut guard, next);
//!     true
//!   } else {
//!     false
//!   }
//! }
//! ```
//!
//! There are additional examples in the code repository.
//!
//! This crate uses `unsafe` code blocks. It is `no_std`-compatible when compiled
//! without the `std` feature.

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs, missing_debug_implementations, rust_2018_idioms)]

#[cfg(any(test, feature = "alloc"))]
extern crate alloc;

mod error;
pub use self::error::OptionLockError;

mod lock;

pub use self::lock::{OptionGuard, OptionLock};

#[cfg(feature = "alloc")]
mod arc;
#[cfg(feature = "alloc")]
pub use self::arc::{MutexGuardArc, OptionGuardArc};

mod once;
pub use self::once::{Lazy, OnceCell};

mod mutex;
pub use self::mutex::{Mutex, MutexGuard};
