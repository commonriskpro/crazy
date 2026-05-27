use serde::{Deserialize, Serialize};

use super::IntegrityIssue;

/// Summary of a storage integrity verification run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IntegrityReport {
    /// All detected issues, sorted for determinism.
    pub issues: Vec<IntegrityIssue>,
    /// Number of snapshots examined.
    pub snapshots_checked: u64,
    /// `true` iff no issues were detected.
    pub passed: bool,
}

/// Summary of a read-only CAS object verification run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectIntegrityReport {
    /// All detected object issues, sorted for determinism.
    pub issues: Vec<IntegrityIssue>,
    /// Number of object ids returned by the store enumeration.
    pub objects_checked: u64,
    /// `true` iff no issues were detected.
    pub passed: bool,
}
