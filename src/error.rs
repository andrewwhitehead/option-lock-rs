use core::fmt::{self, Display, Formatter};

/// Error returned by failing try-lock operations
#[derive(Debug, PartialEq, Eq)]
pub enum OptionLockError {
    /// The fill state of the lock did not match the requirement
    FillState,
    /// The lock could not be acquired
    Unavailable,
}

impl Display for OptionLockError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::FillState => "OptionLockError(FillState)",
            Self::Unavailable => "OptionLockError(Unavailable)",
        })
    }
}

#[cfg(feature = "std")]
impl ::std::error::Error for OptionLockError {}

/// Error returned by failing mutex lock operations
#[derive(Debug, PartialEq, Eq)]
pub enum MutexLockError {
    /// The lock value was removed
    Poisoned,
    /// The lock could not be acquired
    Unavailable,
}

impl Display for MutexLockError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Poisoned => "MutexLockError(Poisoned)",
            Self::Unavailable => "MutexLockError(Unavailable)",
        })
    }
}

#[cfg(feature = "std")]
impl ::std::error::Error for MutexLockError {}

/// Error returned when a lock has been poisoned
#[derive(Debug, PartialEq, Eq)]
pub struct PoisonError;

impl Display for PoisonError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("PoisonError")
    }
}

#[cfg(feature = "std")]
impl ::std::error::Error for PoisonError {}
