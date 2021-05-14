use std::{
    hint::spin_loop,
    ops::{Deref, DerefMut},
    sync::Arc,
    thread,
};

use option_lock::{OptionLock, SomeGuard};

/// A simple wrapper around an OptionLock which ensures that there
/// is always a stored value
struct TryMutex<T> {
    lock: OptionLock<T>,
}

impl<T> TryMutex<T> {
    pub fn new(value: T) -> Self {
        Self { lock: value.into() }
    }

    pub fn try_lock(&self) -> Option<TryMutexGuard<'_, T>> {
        self.lock.try_get().map(|guard| TryMutexGuard { guard })
    }
}

struct TryMutexGuard<'a, T> {
    guard: SomeGuard<'a, T>,
}

impl<T> Deref for TryMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &*self.guard
    }
}

impl<T> DerefMut for TryMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.guard
    }
}

fn main() {
    let shared = Arc::new(TryMutex::new(0i32));
    let threads = 100;
    for _ in 0..threads {
        let shared = shared.clone();
        thread::spawn(move || loop {
            if let Some(mut guard) = shared.try_lock() {
                *guard += 1;
                break;
            }
            spin_loop()
        });
    }
    loop {
        if shared.try_lock().map(|guard| *guard) == Some(threads) {
            break;
        }
        spin_loop()
    }
    println!("Completed {} threads", threads);
}
