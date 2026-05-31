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

// ── Sync diagnostics ─────────────────────────────────────────────────────

/// Stable synchronization primitive names for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SyncPrimitive {
    Mutex,
    RwLock,
}

impl SyncPrimitive {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mutex => "mutex",
            Self::RwLock => "rwlock",
        }
    }
}

/// Stable synchronization operation names for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SyncOperation {
    With,
    Read,
    Write,
}

impl SyncOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::With => "with",
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// Stable synchronization issue kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SyncIssueKind {
    Poisoned,
}

impl SyncIssueKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Poisoned => "std.sync.lock.poisoned",
        }
    }

    pub const fn category(self) -> &'static str {
        match self {
            Self::Poisoned => "runtime-state",
        }
    }
}

/// Machine-readable, redacted synchronization issue descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncIssue {
    pub primitive: SyncPrimitive,
    pub operation: SyncOperation,
    pub kind: SyncIssueKind,
}

impl SyncIssue {
    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }

    pub const fn category(&self) -> &'static str {
        self.kind.category()
    }

    pub const fn primitive_name(&self) -> &'static str {
        self.primitive.as_str()
    }

    pub const fn operation_name(&self) -> &'static str {
        self.operation.as_str()
    }
}

const fn sync_issue(primitive: SyncPrimitive, operation: SyncOperation) -> SyncIssue {
    SyncIssue {
        primitive,
        operation,
        kind: SyncIssueKind::Poisoned,
    }
}

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
        self.try_with(f)
            .unwrap_or_else(|issue| panic!("{}", issue.code()))
    }

    /// Lock the mutex with a stable, redacted poison diagnostic.
    pub fn try_with<F, R>(&self, f: F) -> Result<R, SyncIssue>
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| sync_issue(SyncPrimitive::Mutex, SyncOperation::With))?;
        Ok(f(&mut guard))
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
        self.try_read(f)
            .unwrap_or_else(|issue| panic!("{}", issue.code()))
    }

    /// Acquire a read lock with a stable, redacted poison diagnostic.
    pub fn try_read<F, R>(&self, f: F) -> Result<R, SyncIssue>
    where
        F: FnOnce(&T) -> R,
    {
        let guard = self
            .0
            .read()
            .map_err(|_| sync_issue(SyncPrimitive::RwLock, SyncOperation::Read))?;
        Ok(f(&guard))
    }

    /// Acquire a write lock and apply `f`.
    pub fn write<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        self.try_write(f)
            .unwrap_or_else(|issue| panic!("{}", issue.code()))
    }

    /// Acquire a write lock with a stable, redacted poison diagnostic.
    pub fn try_write<F, R>(&self, f: F) -> Result<R, SyncIssue>
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut guard = self
            .0
            .write()
            .map_err(|_| sync_issue(SyncPrimitive::RwLock, SyncOperation::Write))?;
        Ok(f(&mut guard))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutex_poison_returns_stable_redacted_issue() {
        let mutex = AilMutex::new(7_i64);
        let raw = Arc::clone(&mutex.0);

        let _ = std::thread::spawn(move || {
            let _guard = raw.lock().unwrap();
            panic!("poison mutex for diagnostic test");
        })
        .join();

        let issue = mutex.try_with(|value| *value += 1).unwrap_err();

        assert_eq!(issue.code(), "std.sync.lock.poisoned");
        assert_eq!(issue.category(), "runtime-state");
        assert_eq!(issue.primitive_name(), "mutex");
        assert_eq!(issue.operation_name(), "with");
    }

    #[test]
    fn rwlock_poison_reports_operation_specific_issue() {
        let lock = AilRwLock::new(String::from("secret-state"));
        let raw = Arc::clone(&lock.0);

        let _ = std::thread::spawn(move || {
            let mut guard = raw.write().unwrap();
            guard.push_str("-mutated");
            panic!("poison rwlock for diagnostic test");
        })
        .join();

        let read_issue = lock.try_read(|value| value.len()).unwrap_err();
        assert_eq!(read_issue.code(), "std.sync.lock.poisoned");
        assert_eq!(read_issue.primitive_name(), "rwlock");
        assert_eq!(read_issue.operation_name(), "read");

        let write_issue = lock.try_write(|value| value.clear()).unwrap_err();
        assert_eq!(write_issue.code(), "std.sync.lock.poisoned");
        assert_eq!(write_issue.primitive_name(), "rwlock");
        assert_eq!(write_issue.operation_name(), "write");
    }
}
