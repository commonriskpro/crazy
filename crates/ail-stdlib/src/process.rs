// ── ail-stdlib::process ───────────────────────────────────────────────────
//
// Process management types for the AIL `std.process` module.
//
// # Capabilities (from docs/stdlib.md)
//
// - process.spawn
// - process.signal
//
// # Rules
//
// - process/env are sensitive capabilities
// - prod/critical require strict grants

// ── ExitCode ──────────────────────────────────────────────────────────────

/// A process exit code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitCode(pub i32);

impl ExitCode {
    pub fn success() -> Self {
        Self(0)
    }
    pub fn failure() -> Self {
        Self(1)
    }
    pub fn is_success(self) -> bool {
        self.0 == 0
    }
}

impl std::fmt::Display for ExitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── ProcessId ─────────────────────────────────────────────────────────────

/// A process identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProcessId(pub u32);

// ── ProcessHandle ─────────────────────────────────────────────────────────

/// A handle to a spawned process.
///
/// In the AIL model, `ProcessHandle` is a capability-gated resource.
/// Spawning requires `process.spawn`; signaling requires `process.signal`.
#[derive(Debug)]
pub struct ProcessHandle {
    pub id: ProcessId,
    pub command: String,
}

impl ProcessHandle {
    pub fn new(id: ProcessId, command: impl Into<String>) -> Self {
        Self {
            id,
            command: command.into(),
        }
    }
}

// ── ProcessError ──────────────────────────────────────────────────────────

/// Error from process operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessError {
    PermissionDenied,
    NotFound(String),
    SpawnFailed(String),
    SignalFailed(String),
    Other(String),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessError::PermissionDenied => write!(f, "process permission denied"),
            ProcessError::NotFound(cmd) => write!(f, "command not found: {cmd}"),
            ProcessError::SpawnFailed(msg) => write!(f, "spawn failed: {msg}"),
            ProcessError::SignalFailed(msg) => write!(f, "signal failed: {msg}"),
            ProcessError::Other(msg) => write!(f, "process error: {msg}"),
        }
    }
}
impl std::error::Error for ProcessError {}

// ── Signal ────────────────────────────────────────────────────────────────

/// A POSIX-compatible signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Terminate,
    Kill,
    Interrupt,
    Hangup,
    User1,
    User2,
}
