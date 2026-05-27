use serde::{Deserialize, Serialize};

use crate::advisory::AdvisorySeverity;
use crate::signing::SignedPackage;

/// Request to publish a signed package to the remote registry.
///
/// The caller must supply a `SignedPackage`; the registry verifies the
/// signature before accepting the package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishRequest {
    /// The signed package to publish.
    pub signed_package: SignedPackage,
}

/// Response from a publish request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishResponse {
    /// Whether the publish succeeded.
    pub accepted: bool,
    /// Error message if the publish failed (None on success).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Transparency log entry ID assigned by the registry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_id: Option<String>,
    /// Sequence number in the transparency log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
}

// ── Fetch ─────────────────────────────────────────────────────────────────

/// Request to fetch a specific package version from the remote registry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchRequest {
    /// Package name (e.g., `"payments.stripe"`).
    pub name: String,
    /// Exact version string to fetch (e.g., `"1.2.0"`).
    pub version: String,
}

/// Response from a fetch request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchResponse {
    /// The fetched signed package, or None if not found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_package: Option<SignedPackage>,
    /// Whether the package is yanked.
    pub yanked: bool,
    /// Error message if the fetch failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ── Search ────────────────────────────────────────────────────────────────

/// Request to search for packages in the remote registry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    /// Query string — matched against package name prefix or keyword.
    pub query: String,
    /// Maximum number of results to return (None = registry default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One result entry from a search.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    /// Package name.
    pub name: String,
    /// Latest available version.
    pub latest_version: String,
    /// Short description (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Response from a search request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResponse {
    /// Matching packages.
    pub results: Vec<SearchResult>,
    /// Whether the result list was truncated by the server.
    pub truncated: bool,
}

// ── Verify ────────────────────────────────────────────────────────────────

/// Request to verify a package's integrity and advisory status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyRequest {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// BLAKE3 hex digest the client expects for this package.
    pub expected_hash: String,
}

/// Verification outcome from the registry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifyOutcome {
    /// Package matches and has no active advisories.
    Ok,
    /// Package hash does not match — possible tampering.
    HashMismatch {
        /// The hash the registry computed.
        registry_hash: String,
    },
    /// Package is covered by one or more active advisories.
    Advisory {
        /// First matching advisory ID.
        advisory_id: String,
        /// Severity of the advisory.
        severity: AdvisorySeverity,
    },
    /// Package was not found in the registry.
    NotFound,
    /// Package has been yanked.
    Yanked {
        /// Reason for yanking.
        reason: String,
    },
}

/// Response from a verify request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyResponse {
    /// The verification outcome.
    pub outcome: VerifyOutcome,
}
