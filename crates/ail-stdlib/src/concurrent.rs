// ── ail-stdlib::concurrent ────────────────────────────────────────────────
//
// Task and channel primitive types for the AIL `std.concurrent` module.
//
// # Rules (from docs/stdlib.md)
//
// - structured concurrency by default
// - no orphan tasks
// - shared state requires safe type/capability
// - timeouts use clock.monotonic
//
// This module provides type definitions and policy markers. Actual async
// execution is provided by the AIL runtime host.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

// ── CancellationToken ─────────────────────────────────────────────────────

/// A token that can be used to request cancellation of a task or group.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<Mutex<bool>>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(Mutex::new(false)),
        }
    }

    /// Request cancellation.
    pub fn cancel(&self) {
        if let Ok(mut c) = self.cancelled.lock() {
            *c = true;
        }
    }

    /// Return `true` if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.lock().map(|c| *c).unwrap_or(false)
    }
}

// ── TaskGroup ─────────────────────────────────────────────────────────────

/// A structured group of tasks. No task may outlive its `TaskGroup`.
///
/// In the AIL model, tasks must be explicitly joined before the group is
/// dropped — no orphan tasks.
#[derive(Debug, Default)]
pub struct TaskGroup {
    pub token: CancellationToken,
    pub task_count: usize,
}

impl TaskGroup {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new task in the group (in a real runtime, would spawn).
    pub fn spawn(&mut self) {
        self.task_count += 1;
    }

    /// Signal all tasks in the group to stop.
    pub fn cancel_all(&self) {
        self.token.cancel();
    }
}

// ── Channel ───────────────────────────────────────────────────────────────

/// A bounded channel for communication between tasks.
///
/// `send` returns `Err` if the channel is full.
/// `recv` returns `None` if the channel is empty.
#[derive(Debug)]
pub struct Channel<T> {
    queue: Arc<Mutex<VecDeque<T>>>,
    capacity: usize,
}

impl<T> Channel<T> {
    /// Create a new channel with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            capacity,
        }
    }

    /// Send a value. Returns `Err(value)` if the channel is full.
    pub fn send(&self, value: T) -> Result<(), T> {
        let mut q = self.queue.lock().unwrap();
        if q.len() >= self.capacity {
            return Err(value);
        }
        q.push_back(value);
        Ok(())
    }

    /// Receive a value. Returns `None` if the channel is empty.
    pub fn recv(&self) -> Option<T> {
        let mut q = self.queue.lock().unwrap();
        q.pop_front()
    }

    /// Return the number of items currently in the channel.
    pub fn len(&self) -> usize {
        self.queue.lock().unwrap().len()
    }

    /// Return `true` if the channel is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Clone for Channel<T> {
    fn clone(&self) -> Self {
        Self {
            queue: Arc::clone(&self.queue),
            capacity: self.capacity,
        }
    }
}

// ── Timeout ───────────────────────────────────────────────────────────────

/// An explicit timeout. Uses `clock.monotonic` in a real runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeout {
    pub millis: u64,
}

impl Timeout {
    pub fn from_millis(ms: u64) -> Self {
        Self { millis: ms }
    }
    pub fn from_secs(s: u64) -> Self {
        Self { millis: s * 1000 }
    }
}
