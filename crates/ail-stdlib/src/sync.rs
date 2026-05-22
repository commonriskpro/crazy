// ── ail-stdlib::sync ──────────────────────────────────────────────────────
//
// Synchronization primitives for the AIL `std.sync` module.
//
// # Rules (from docs/stdlib.md)
//
// - structured concurrency by default
// - shared state requires safe type/capability

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};

// ── AilMutex ─────────────────────────────────────────────────────────────

/// A mutual-exclusion lock protecting a value of type `T`.
///
/// Wraps `std::sync::Mutex` with an AIL-flavored API.
#[derive(Debug, Default)]
pub struct AilMutex<T>(pub Arc<StdMutex<T>>);

impl<T> AilMutex<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(StdMutex::new(value)))
    }

    /// Lock the mutex and apply `f` to the contained value.
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut guard = self.0.lock().expect("mutex poisoned");
        f(&mut guard)
    }
}

impl<T> Clone for AilMutex<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

// ── AilRwLock ─────────────────────────────────────────────────────────────

/// A readers-writer lock protecting a value of type `T`.
#[derive(Debug, Default)]
pub struct AilRwLock<T>(pub Arc<StdRwLock<T>>);

impl<T> AilRwLock<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(StdRwLock::new(value)))
    }

    /// Acquire a read lock and apply `f`.
    pub fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        let guard = self.0.read().expect("rwlock poisoned");
        f(&guard)
    }

    /// Acquire a write lock and apply `f`.
    pub fn write<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut guard = self.0.write().expect("rwlock poisoned");
        f(&mut guard)
    }
}

impl<T> Clone for AilRwLock<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

// ── AilAtomicBool ─────────────────────────────────────────────────────────

/// An atomic boolean.
#[derive(Debug, Default)]
pub struct AilAtomicBool(Arc<AtomicBool>);

impl AilAtomicBool {
    pub fn new(v: bool) -> Self {
        Self(Arc::new(AtomicBool::new(v)))
    }
    pub fn load(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn store(&self, v: bool) {
        self.0.store(v, Ordering::SeqCst);
    }
    pub fn swap(&self, v: bool) -> bool {
        self.0.swap(v, Ordering::SeqCst)
    }
}

impl Clone for AilAtomicBool {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

// ── AilAtomicI64 ──────────────────────────────────────────────────────────

/// An atomic i64.
#[derive(Debug, Default)]
pub struct AilAtomicI64(Arc<AtomicI64>);

impl AilAtomicI64 {
    pub fn new(v: i64) -> Self {
        Self(Arc::new(AtomicI64::new(v)))
    }
    pub fn load(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
    pub fn store(&self, v: i64) {
        self.0.store(v, Ordering::SeqCst);
    }
    pub fn fetch_add(&self, v: i64) -> i64 {
        self.0.fetch_add(v, Ordering::SeqCst)
    }
    pub fn fetch_sub(&self, v: i64) -> i64 {
        self.0.fetch_sub(v, Ordering::SeqCst)
    }
    pub fn compare_exchange(&self, current: i64, new: i64) -> Result<i64, i64> {
        self.0
            .compare_exchange(current, new, Ordering::SeqCst, Ordering::SeqCst)
    }
}

impl Clone for AilAtomicI64 {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}
