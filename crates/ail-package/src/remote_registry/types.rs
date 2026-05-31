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

// ── Diagnostics ────────────────────────────────────────────────────────────

/// Remote registry operation associated with a diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RemoteRegistryOperation {
    /// Publishing a signed package.
    Publish,
    /// Fetching a package payload.
    Fetch,
    /// Searching registry metadata.
    Search,
    /// Verifying package integrity/advisory status.
    Verify,
    /// Refreshing or validating registry index metadata.
    Index,
}

/// Stable machine-readable remote registry diagnostic kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RemoteRegistryDiagnosticKind {
    /// The registry endpoint was unavailable to the client.
    RegistryUnavailable,
    /// Registry authentication or authorization was denied.
    AuthDenied,
    /// The registry returned a response that could not be decoded.
    MalformedResponse,
    /// Index metadata points at package content that is not available locally.
    StaleIndex,
    /// Multiple index entries represent the same package version.
    DuplicatePublishVersion,
}

impl RemoteRegistryDiagnosticKind {
    /// Stable issue code for production dashboards and automation.
    pub fn code(self) -> &'static str {
        match self {
            RemoteRegistryDiagnosticKind::RegistryUnavailable => "REMOTE_REGISTRY_UNAVAILABLE",
            RemoteRegistryDiagnosticKind::AuthDenied => "REMOTE_REGISTRY_AUTH_DENIED",
            RemoteRegistryDiagnosticKind::MalformedResponse => "REMOTE_REGISTRY_MALFORMED_RESPONSE",
            RemoteRegistryDiagnosticKind::StaleIndex => "REMOTE_REGISTRY_STALE_INDEX",
            RemoteRegistryDiagnosticKind::DuplicatePublishVersion => {
                "REMOTE_REGISTRY_DUPLICATE_PUBLISH_VERSION"
            }
        }
    }

    /// Stable low-cardinality category for grouping diagnostics.
    pub fn category(self) -> &'static str {
        match self {
            RemoteRegistryDiagnosticKind::RegistryUnavailable => "availability",
            RemoteRegistryDiagnosticKind::AuthDenied => "authentication",
            RemoteRegistryDiagnosticKind::MalformedResponse => "protocol",
            RemoteRegistryDiagnosticKind::StaleIndex => "index_integrity",
            RemoteRegistryDiagnosticKind::DuplicatePublishVersion => "publish_integrity",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            RemoteRegistryDiagnosticKind::RegistryUnavailable => "registry unavailable",
            RemoteRegistryDiagnosticKind::AuthDenied => "registry authentication denied",
            RemoteRegistryDiagnosticKind::MalformedResponse => "registry response malformed",
            RemoteRegistryDiagnosticKind::StaleIndex => {
                "registry index references missing package content"
            }
            RemoteRegistryDiagnosticKind::DuplicatePublishVersion => {
                "registry index contains duplicate package version entries"
            }
        }
    }
}

/// Redaction guarantees attached to a remote registry diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRegistryDiagnosticRedaction {
    /// Raw registry URLs are intentionally omitted.
    pub registry_url: bool,
    /// Raw auth credentials or bearer tokens are intentionally omitted.
    pub auth_token: bool,
    /// Raw package names and versions are intentionally omitted.
    pub package_coordinate: bool,
}

impl RemoteRegistryDiagnosticRedaction {
    fn all() -> Self {
        Self {
            registry_url: true,
            auth_token: true,
            package_coordinate: true,
        }
    }
}

/// Stable, redacted production diagnostic for remote registry workflows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRegistryDiagnostic {
    /// Stable machine-readable issue kind.
    pub kind: RemoteRegistryDiagnosticKind,
    /// Stable issue code for dashboards and policy gates.
    pub code: String,
    /// Stable low-cardinality category.
    pub category: String,
    /// Registry operation that surfaced the issue.
    pub operation: RemoteRegistryOperation,
    /// Stable human-readable summary without raw URLs, tokens, or coordinates.
    pub summary: String,
    /// Redaction guarantees for sensitive dimensions.
    pub redaction: RemoteRegistryDiagnosticRedaction,
    /// Deterministic ordinal among diagnostics with otherwise identical public fields.
    pub ordinal: usize,
}

impl RemoteRegistryDiagnostic {
    /// Create a diagnostic from a registry-unavailable transport failure.
    pub fn registry_unavailable(
        operation: RemoteRegistryOperation,
        _registry_url: impl AsRef<str>,
    ) -> Self {
        Self::new(
            RemoteRegistryDiagnosticKind::RegistryUnavailable,
            operation,
            0,
        )
    }

    /// Create a diagnostic from an authentication/authorization denial.
    pub fn auth_denied(
        operation: RemoteRegistryOperation,
        _registry_url: impl AsRef<str>,
        _auth_token: impl AsRef<str>,
    ) -> Self {
        Self::new(RemoteRegistryDiagnosticKind::AuthDenied, operation, 0)
    }

    /// Create a diagnostic from an undecodable registry response.
    pub fn malformed_response(
        operation: RemoteRegistryOperation,
        _registry_url: impl AsRef<str>,
        _response_body: impl AsRef<[u8]>,
    ) -> Self {
        Self::new(
            RemoteRegistryDiagnosticKind::MalformedResponse,
            operation,
            0,
        )
    }

    pub(crate) fn stale_index(ordinal: usize) -> Self {
        Self::new(
            RemoteRegistryDiagnosticKind::StaleIndex,
            RemoteRegistryOperation::Index,
            ordinal,
        )
    }

    pub(crate) fn duplicate_publish_version(ordinal: usize) -> Self {
        Self::new(
            RemoteRegistryDiagnosticKind::DuplicatePublishVersion,
            RemoteRegistryOperation::Index,
            ordinal,
        )
    }

    fn new(
        kind: RemoteRegistryDiagnosticKind,
        operation: RemoteRegistryOperation,
        ordinal: usize,
    ) -> Self {
        Self {
            kind,
            code: kind.code().to_string(),
            category: kind.category().to_string(),
            operation,
            summary: kind.summary().to_string(),
            redaction: RemoteRegistryDiagnosticRedaction::all(),
            ordinal,
        }
    }
}

/// Deterministically ordered remote registry diagnostic report.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRegistryDiagnosticReport {
    /// Sorted diagnostics.
    pub diagnostics: Vec<RemoteRegistryDiagnostic>,
}

impl RemoteRegistryDiagnosticReport {
    /// Build a report and sort diagnostics by stable public fields.
    pub fn from_diagnostics(mut diagnostics: Vec<RemoteRegistryDiagnostic>) -> Self {
        sort_remote_registry_diagnostics(&mut diagnostics);
        Self { diagnostics }
    }

    /// Return true when no diagnostics were emitted.
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

fn sort_remote_registry_diagnostics(diagnostics: &mut [RemoteRegistryDiagnostic]) {
    diagnostics
        .sort_by(|a, b| remote_registry_diagnostic_key(a).cmp(&remote_registry_diagnostic_key(b)));
}

fn remote_registry_diagnostic_key(
    diagnostic: &RemoteRegistryDiagnostic,
) -> (&str, RemoteRegistryOperation, &str, usize) {
    (
        diagnostic.code.as_str(),
        diagnostic.operation,
        diagnostic.category.as_str(),
        diagnostic.ordinal,
    )
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
