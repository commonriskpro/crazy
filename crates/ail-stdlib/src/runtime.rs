// ── ail-stdlib::runtime ───────────────────────────────────────────────────
//
// Runtime-facing types for the AIL `std.runtime` module.

// ── RuntimeProfile ────────────────────────────────────────────────────────

/// Identifies a runtime execution profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeProfile {
    /// Development — relaxed limits, verbose errors.
    Development,
    /// Staging — production-like but with extra observability.
    Staging,
    /// Production — strict limits, minimal surface.
    Production,
    /// Critical — highest restriction level.
    Critical,
    /// Custom profile identified by name.
    Custom(String),
}

impl std::fmt::Display for RuntimeProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeProfile::Development => write!(f, "development"),
            RuntimeProfile::Staging => write!(f, "staging"),
            RuntimeProfile::Production => write!(f, "production"),
            RuntimeProfile::Critical => write!(f, "critical"),
            RuntimeProfile::Custom(s) => write!(f, "custom:{s}"),
        }
    }
}

// ── LimitConfig ───────────────────────────────────────────────────────────

/// Resource limits for a runtime execution context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitConfig {
    /// Maximum fuel (computation steps).
    pub max_fuel: Option<u64>,
    /// Maximum memory in bytes.
    pub max_memory_bytes: Option<u64>,
    /// Maximum wall-clock execution time in milliseconds.
    pub max_time_millis: Option<u64>,
    /// Maximum number of spawned tasks.
    pub max_tasks: Option<u32>,
}

impl Default for LimitConfig {
    fn default() -> Self {
        Self {
            max_fuel: None,
            max_memory_bytes: None,
            max_time_millis: None,
            max_tasks: None,
        }
    }
}

impl LimitConfig {
    pub fn unlimited() -> Self {
        Self::default()
    }

    pub fn strict() -> Self {
        Self {
            max_fuel: Some(10_000_000),
            max_memory_bytes: Some(64 * 1024 * 1024),
            max_time_millis: Some(30_000),
            max_tasks: Some(64),
        }
    }
}

// ── AuditEvent ────────────────────────────────────────────────────────────

/// A runtime audit event (not an app log).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEvent {
    pub id: String,
    pub event_type: String,
    pub module: String,
    pub capability: Option<String>,
    pub timestamp_secs: i64,
    pub outcome: AuditOutcome,
}

/// Outcome of an auditable operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditOutcome {
    Allowed,
    Denied,
    Error,
}

impl AuditEvent {
    pub fn new(
        id: impl Into<String>,
        event_type: impl Into<String>,
        module: impl Into<String>,
        timestamp_secs: i64,
        outcome: AuditOutcome,
    ) -> Self {
        Self {
            id: id.into(),
            event_type: event_type.into(),
            module: module.into(),
            capability: None,
            timestamp_secs,
            outcome,
        }
    }
}

// ── RuntimeReport ─────────────────────────────────────────────────────────

/// A report generated after a runtime execution.
#[derive(Clone, Debug, Default)]
pub struct RuntimeReport {
    pub profile: Option<RuntimeProfile>,
    pub limits: LimitConfig,
    pub fuel_consumed: Option<u64>,
    pub memory_peak_bytes: Option<u64>,
    pub audit_events: Vec<AuditEvent>,
    pub exit_ok: bool,
}

impl RuntimeReport {
    pub fn new() -> Self {
        Self {
            exit_ok: true,
            ..Default::default()
        }
    }
}

// ── ReplayConfig ──────────────────────────────────────────────────────────

/// Configuration for deterministic execution replay.
#[derive(Clone, Debug, Default)]
pub struct ReplayConfig {
    pub seed: Option<u64>,
    pub capture_io: bool,
    pub replay_io: bool,
}

// ── ArtifactManifest ──────────────────────────────────────────────────────

/// Manifest describing the artifacts produced by a build or runtime run.
#[derive(Clone, Debug, Default)]
pub struct ArtifactManifest {
    pub artifacts: Vec<ArtifactEntry>,
}

/// A single artifact entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactEntry {
    pub id: String,
    pub path: String,
    pub hash: Option<String>,
    pub size_bytes: Option<u64>,
}

impl ArtifactManifest {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, entry: ArtifactEntry) {
        self.artifacts.push(entry);
    }
}
