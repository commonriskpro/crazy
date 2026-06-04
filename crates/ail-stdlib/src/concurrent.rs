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

// ── Channel diagnostics ──────────────────────────────────────────────────

/// Stable operation names for `std.concurrent.Channel` diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChannelOperation {
    Send,
    Recv,
    Len,
}

impl ChannelOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Recv => "recv",
            Self::Len => "len",
        }
    }
}

/// Stable, redacted issue kinds for bounded channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChannelIssueKind {
    Full,
    Poisoned,
}

impl ChannelIssueKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Full => "std.concurrent.channel.full",
            Self::Poisoned => "std.concurrent.channel.poisoned",
        }
    }

    pub const fn category(self) -> &'static str {
        match self {
            Self::Full => "capacity",
            Self::Poisoned => "runtime-state",
        }
    }
}

/// Machine-readable channel issue descriptor.
///
/// It intentionally exposes capacity/length shape, not queued values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelIssue {
    pub operation: ChannelOperation,
    pub kind: ChannelIssueKind,
    pub capacity: usize,
    pub len: Option<usize>,
}

impl ChannelIssue {
    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }

    pub const fn category(&self) -> &'static str {
        self.kind.category()
    }

    pub const fn operation_name(&self) -> &'static str {
        self.operation.as_str()
    }
}

/// Send failure that preserves the unsent value while keeping diagnostics redacted.
#[derive(Debug, PartialEq, Eq)]
pub enum ChannelSendError<T> {
    Full { value: T, issue: ChannelIssue },
    Issue { value: T, issue: ChannelIssue },
}

impl<T> ChannelSendError<T> {
    pub const fn issue(&self) -> &ChannelIssue {
        match self {
            Self::Full { issue, .. } | Self::Issue { issue, .. } => issue,
        }
    }

    pub fn into_value(self) -> T {
        match self {
            Self::Full { value, .. } | Self::Issue { value, .. } => value,
        }
    }
}

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

    fn issue(
        &self,
        operation: ChannelOperation,
        kind: ChannelIssueKind,
        len: Option<usize>,
    ) -> ChannelIssue {
        ChannelIssue {
            operation,
            kind,
            capacity: self.capacity,
            len,
        }
    }

    /// Send a value. Returns `Err(value)` if the channel is full or poisoned.
    pub fn send(&self, value: T) -> Result<(), T> {
        self.try_send(value).map_err(ChannelSendError::into_value)
    }

    /// Send a value with a machine-readable, redacted failure descriptor.
    pub fn try_send(&self, value: T) -> Result<(), ChannelSendError<T>> {
        let mut q = match self.queue.lock() {
            Ok(guard) => guard,
            Err(_) => {
                return Err(ChannelSendError::Issue {
                    value,
                    issue: self.issue(ChannelOperation::Send, ChannelIssueKind::Poisoned, None),
                });
            }
        };
        if q.len() >= self.capacity {
            return Err(ChannelSendError::Full {
                value,
                issue: self.issue(
                    ChannelOperation::Send,
                    ChannelIssueKind::Full,
                    Some(q.len()),
                ),
            });
        }
        q.push_back(value);
        Ok(())
    }

    /// Receive a value. Returns `None` if the channel is empty or poisoned.
    pub fn recv(&self) -> Option<T> {
        self.try_recv().ok().flatten()
    }

    /// Receive a value with a machine-readable poison diagnostic.
    pub fn try_recv(&self) -> Result<Option<T>, ChannelIssue> {
        let mut q = self
            .queue
            .lock()
            .map_err(|_| self.issue(ChannelOperation::Recv, ChannelIssueKind::Poisoned, None))?;
        Ok(q.pop_front())
    }

    /// Return the number of items currently in the channel.
    pub fn len(&self) -> usize {
        self.try_len().unwrap_or(0)
    }

    /// Return the number of items with a machine-readable poison diagnostic.
    pub fn try_len(&self) -> Result<usize, ChannelIssue> {
        self.queue
            .lock()
            .map(|q| q.len())
            .map_err(|_| self.issue(ChannelOperation::Len, ChannelIssueKind::Poisoned, None))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_channel_returns_redacted_capacity_issue() {
        let channel = Channel::new(1);
        assert_eq!(channel.try_send("first"), Ok(()));

        let err = channel.try_send("secret-payload").unwrap_err();
        let issue = *err.issue();

        assert_eq!(err.into_value(), "secret-payload");
        assert_eq!(issue.code(), "std.concurrent.channel.full");
        assert_eq!(issue.category(), "capacity");
        assert_eq!(issue.operation_name(), "send");
        assert_eq!(issue.capacity, 1);
        assert_eq!(issue.len, Some(1));
    }

    #[test]
    fn poisoned_channel_returns_stable_issue_without_payload_leak() {
        let channel = Channel::new(2);
        let queue = Arc::clone(&channel.queue);

        let _ = std::thread::spawn(move || {
            let _guard = queue.lock().unwrap();
            panic!("poison channel for diagnostic test");
        })
        .join();

        let err = channel.try_send("classified").unwrap_err();
        let issue = *err.issue();

        assert_eq!(err.into_value(), "classified");
        assert_eq!(issue.code(), "std.concurrent.channel.poisoned");
        assert_eq!(issue.category(), "runtime-state");
        assert_eq!(issue.operation_name(), "send");
        assert_eq!(issue.capacity, 2);
        assert_eq!(issue.len, None);

        let len_issue = channel.try_len().unwrap_err();
        assert_eq!(len_issue.operation_name(), "len");
        assert_eq!(len_issue.code(), "std.concurrent.channel.poisoned");
    }
}
