// ── ail-verify::report ────────────────────────────────────────────────────
//
// Pure value types for the verification report layer.
//
// # Six verification states
//
// | State          | Meaning                                                  |
// |----------------|----------------------------------------------------------|
// | `Proven`       | Directly established by declared nominal type facts.     |
// | `RuntimeChecked` | Validated by a runtime assertion (future phase).       |
// | `Assumed`      | Declared by the programmer (effect/capability present).  |
// | `Unverified`   | No fact declared; status unknown.                        |
// | `Unsafe`       | Explicitly marked as unsafe / unsound.                   |
// | `Failed`       | A verification condition is violated.                    |
//
// # Summary priority
//
// `Failed > Unsafe > Unverified > Assumed > RuntimeChecked > Proven`
//
// An empty report has vacuous summary `Proven`.
//
// # Report schema
//
// `VerificationReport` carries a `schema_version` ("verification/1.0"), an
// optional `profile`, and pre-computed `summary_counts` so consumers do not
// need to re-iterate entries.

use serde::{Deserialize, Serialize};

// ── VerificationState ─────────────────────────────────────────────────────

/// The result of checking one claim in a `VerificationEntry`.
///
/// Exactly six variants are permitted in Phase 5.  Exhaustive matches
/// elsewhere in the codebase will break if variants are added, which is
/// intentional — it forces callers to handle new states explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationState {
    /// Claim directly established via declared nominal type facts.
    Proven,
    /// Claim validated by a runtime assertion (reserved for future phase).
    RuntimeChecked,
    /// Claim declared by the programmer; correctness not yet mechanically proven.
    Assumed,
    /// No fact was declared; status unknown.
    Unverified,
    /// Explicitly marked as unsafe or potentially unsound.
    Unsafe,
    /// A verification condition is known to be violated.
    Failed,
}

impl VerificationState {
    /// Numeric priority used by `VerificationReport::summary()`.
    ///
    /// Higher value = higher severity.  `Proven` has priority 0 (lowest).
    fn priority(self) -> u8 {
        match self {
            VerificationState::Proven => 0,
            VerificationState::RuntimeChecked => 1,
            VerificationState::Assumed => 2,
            VerificationState::Unverified => 3,
            VerificationState::Unsafe => 4,
            VerificationState::Failed => 5,
        }
    }
}

// ── VerificationEntry ─────────────────────────────────────────────────────

/// One verification claim for a single fact in a `GraphNode`.
///
/// `evidence` is `Option<String>` so that absent evidence is omitted from
/// CBOR output rather than serialized as an empty string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationEntry {
    /// Human-readable description of the claim being checked.
    pub claim: String,
    /// Verification result for this claim.
    pub state: VerificationState,
    /// Scope identifier (typically a node name or id).
    pub scope: String,
    /// Optional supporting evidence or rationale.
    ///
    /// Serialized with `skip_serializing_if` so that `None` is absent
    /// (not `null` or `""`) in CBOR output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

// ── SummaryCounts ─────────────────────────────────────────────────────────

/// Pre-computed per-state entry counts for one `VerificationReport`.
///
/// Mirrors the `summary` block from the verification report schema
/// (`docs/verification.md` §Verification report schema).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryCounts {
    /// Entries in state `Proven`.
    pub verified_count: u32,
    /// Entries in state `RuntimeChecked`.
    pub runtime_checked_count: u32,
    /// Entries in state `Assumed`.
    pub assumed_count: u32,
    /// Entries in state `Unverified`.
    pub unverified_count: u32,
    /// Entries in state `Unsafe`.
    pub unsafe_count: u32,
    /// Entries in state `Failed`.
    pub failed_count: u32,
}

impl SummaryCounts {
    /// Compute `SummaryCounts` from a slice of `VerificationEntry` items.
    ///
    /// Each entry's `state` is counted once.  The counts are non-overlapping
    /// and sum to `entries.len()`.
    pub fn from_entries(entries: &[VerificationEntry]) -> Self {
        let mut counts = SummaryCounts::default();
        for entry in entries {
            match entry.state {
                VerificationState::Proven => counts.verified_count += 1,
                VerificationState::RuntimeChecked => counts.runtime_checked_count += 1,
                VerificationState::Assumed => counts.assumed_count += 1,
                VerificationState::Unverified => counts.unverified_count += 1,
                VerificationState::Unsafe => counts.unsafe_count += 1,
                VerificationState::Failed => counts.failed_count += 1,
            }
        }
        counts
    }
}

// ── VerificationReport ────────────────────────────────────────────────────

/// The canonical report schema version.
pub const SCHEMA_VERSION: &str = "verification/1.0";

/// Ordered collection of verification entries for one `SemanticGraph` pass.
///
/// Iteration order is preserved from the graph traversal, guaranteeing
/// deterministic output across runs.
///
/// # Schema enrichment
///
/// `schema_version` is always `"verification/1.0"`.  `profile` is the build
/// profile under which verification ran (e.g. `"prod"`, `"dev"`).
/// `summary_counts` mirrors the `summary` block in the verification report
/// schema and is automatically derived from `entries` by `VerificationReport::new`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReport {
    /// Verification entries in graph traversal order.
    pub entries: Vec<VerificationEntry>,
    /// Report schema version — always `"verification/1.0"`.
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    /// Optional build profile under which this report was produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Pre-computed per-state counts (derived from `entries`).
    #[serde(default)]
    pub summary_counts: SummaryCounts,
}

impl Default for VerificationReport {
    fn default() -> Self {
        Self {
            entries: vec![],
            schema_version: SCHEMA_VERSION.to_string(),
            profile: None,
            summary_counts: SummaryCounts::default(),
        }
    }
}

fn default_schema_version() -> String {
    SCHEMA_VERSION.to_string()
}

impl VerificationReport {
    /// Construct a `VerificationReport` from entries, computing `summary_counts`
    /// automatically.
    ///
    /// Prefer this over direct struct literal construction for new code.
    pub fn new(entries: Vec<VerificationEntry>) -> Self {
        let summary_counts = SummaryCounts::from_entries(&entries);
        Self {
            entries,
            schema_version: SCHEMA_VERSION.to_string(),
            profile: None,
            summary_counts,
        }
    }

    /// Set the profile and return `Self` (builder pattern).
    ///
    /// # Example
    ///
    /// ```rust
    /// use ail_verify::report::VerificationReport;
    ///
    /// let report = VerificationReport::new(vec![]).with_profile("prod");
    /// assert_eq!(report.profile.as_deref(), Some("prod"));
    /// ```
    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// Returns the highest-severity `VerificationState` present in the report.
    ///
    /// An empty report returns `Proven` (vacuous truth — nothing has failed).
    ///
    /// Priority order (descending): `Failed > Unsafe > Unverified > Assumed >
    /// RuntimeChecked > Proven`.
    pub fn summary(&self) -> VerificationState {
        self.entries
            .iter()
            .map(|e| e.state)
            .max_by_key(|s| s.priority())
            .unwrap_or(VerificationState::Proven)
    }
}
