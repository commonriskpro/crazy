use serde::{Deserialize, Serialize};

use crate::manifest::PackageManifest;
use crate::signing::{SignedPackage, SigningError};

/// Stable machine-readable issue kind emitted by signed publish validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SignedPublishIssueKind {
    /// The signed package does not carry a usable publisher key identity.
    MissingPublisherKeyId,
    /// The package manifest could not be hashed for signature verification.
    ManifestHashInvalid,
    /// The Ed25519 signature does not verify for the embedded manifest.
    SignatureInvalid,
}

impl SignedPublishIssueKind {
    /// Stable issue code for downstream audit tooling.
    pub fn code(self) -> &'static str {
        match self {
            SignedPublishIssueKind::MissingPublisherKeyId => "SIGNED_PUBLISH_MISSING_KEY_ID",
            SignedPublishIssueKind::ManifestHashInvalid => "SIGNED_PUBLISH_MANIFEST_HASH_INVALID",
            SignedPublishIssueKind::SignatureInvalid => "SIGNED_PUBLISH_SIGNATURE_INVALID",
        }
    }

    /// Stable category for low-cardinality audit aggregation.
    pub fn category(self) -> &'static str {
        match self {
            SignedPublishIssueKind::MissingPublisherKeyId => "publisher_identity",
            SignedPublishIssueKind::ManifestHashInvalid => "manifest_integrity",
            SignedPublishIssueKind::SignatureInvalid => "signature_integrity",
        }
    }
}

impl std::fmt::Display for SignedPublishIssueKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())
    }
}

/// Redacted publisher key shape for publish audit records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedPublishKeyShape {
    /// Signature algorithm associated with the key material.
    pub algorithm: String,
    /// Raw public-key byte length, without exposing key bytes.
    pub byte_len: usize,
    /// Always true: audit records must not expose raw signing keys.
    pub redacted: bool,
}

impl SignedPublishKeyShape {
    fn from_signed(signed: &SignedPackage) -> Self {
        Self {
            algorithm: "ed25519".to_string(),
            byte_len: signed.sig.signer.len(),
            redacted: true,
        }
    }
}

/// Redacted, stable issue emitted by signed publish validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedPublishIssue {
    /// Stable machine-readable issue kind.
    pub kind: SignedPublishIssueKind,
    /// Stable issue code for downstream audit tooling.
    pub code: String,
    /// Stable category for low-cardinality aggregation.
    pub category: String,
    /// Package name associated with the publish request.
    pub package_name: String,
    /// Package version associated with the publish request.
    pub package_version: String,
    /// Redacted signer key shape; never raw key bytes.
    pub signer_key: SignedPublishKeyShape,
}

impl SignedPublishIssue {
    fn new(kind: SignedPublishIssueKind, signed: &SignedPackage) -> Self {
        Self {
            kind,
            code: kind.code().to_string(),
            category: kind.category().to_string(),
            package_name: signed.manifest.name.clone(),
            package_version: signed.manifest.version.clone(),
            signer_key: SignedPublishKeyShape::from_signed(signed),
        }
    }
}

/// Redacted validation report for a signed publish request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedPublishAuditReport {
    /// Whether the request is eligible for registry insertion.
    pub accepted: bool,
    /// Package name associated with the publish request.
    pub package_name: String,
    /// Package version associated with the publish request.
    pub package_version: String,
    /// Deterministically ordered validation issues.
    pub issues: Vec<SignedPublishIssue>,
}

/// Result of attempting an audited signed publish.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignedPublishResult {
    /// The publish was accepted and inserted into the registry.
    Accepted {
        /// Published manifest.
        manifest: PackageManifest,
        /// Redacted audit report for the accepted request.
        audit: SignedPublishAuditReport,
    },
    /// The publish was rejected before registry insertion.
    Rejected {
        /// Redacted audit report containing stable issue codes.
        audit: SignedPublishAuditReport,
    },
}

impl SignedPublishResult {
    /// Return the audit report for either accepted or rejected publishes.
    pub fn audit(&self) -> &SignedPublishAuditReport {
        match self {
            SignedPublishResult::Accepted { audit, .. }
            | SignedPublishResult::Rejected { audit } => audit,
        }
    }

    /// Return true when the signed publish was accepted.
    pub fn accepted(&self) -> bool {
        matches!(self, SignedPublishResult::Accepted { .. })
    }
}

/// Extension that registers a `SignedPackage` into a `PackageRegistry` after
/// verifying its signature.
///
/// This wires signing into the publish workflow: callers cannot bypass
/// signature verification when publishing through this API.
pub fn publish_signed(
    registry: &mut crate::registry::PackageRegistry,
    signed: &SignedPackage,
) -> Result<PackageManifest, SigningError> {
    match publish_signed_audited(registry, signed) {
        SignedPublishResult::Accepted { manifest, .. } => Ok(manifest),
        SignedPublishResult::Rejected { audit } => Err(signing_error_from_audit(&audit)),
    }
}

/// Register a signed package and return a redacted, stable audit result.
pub fn publish_signed_audited(
    registry: &mut crate::registry::PackageRegistry,
    signed: &SignedPackage,
) -> SignedPublishResult {
    let audit = validate_signed_publish(signed);
    if !audit.accepted {
        return SignedPublishResult::Rejected { audit };
    }

    match registry.register_signed(signed.clone()) {
        Ok(()) => SignedPublishResult::Accepted {
            manifest: signed.manifest.clone(),
            audit,
        },
        Err(error) => SignedPublishResult::Rejected {
            audit: audit_from_signing_error(signed, error),
        },
    }
}

/// Validate a signed publish request without mutating the registry.
pub fn validate_signed_publish(signed: &SignedPackage) -> SignedPublishAuditReport {
    let mut issues = Vec::new();

    if signed.sig.signer.iter().all(|byte| *byte == 0) {
        issues.push(SignedPublishIssue::new(
            SignedPublishIssueKind::MissingPublisherKeyId,
            signed,
        ));
    }

    if let Err(error) = signed.verify() {
        issues.push(SignedPublishIssue::new(
            issue_kind_from_error(&error),
            signed,
        ));
    }

    sort_signed_publish_issues(&mut issues);

    SignedPublishAuditReport {
        accepted: issues.is_empty(),
        package_name: signed.manifest.name.clone(),
        package_version: signed.manifest.version.clone(),
        issues,
    }
}

fn audit_from_signing_error(
    signed: &SignedPackage,
    error: SigningError,
) -> SignedPublishAuditReport {
    let mut issues = vec![SignedPublishIssue::new(
        issue_kind_from_error(&error),
        signed,
    )];
    sort_signed_publish_issues(&mut issues);
    SignedPublishAuditReport {
        accepted: false,
        package_name: signed.manifest.name.clone(),
        package_version: signed.manifest.version.clone(),
        issues,
    }
}

fn signing_error_from_audit(audit: &SignedPublishAuditReport) -> SigningError {
    match audit.issues.first().map(|issue| issue.kind) {
        Some(SignedPublishIssueKind::ManifestHashInvalid) => SigningError::HashError(
            SignedPublishIssueKind::ManifestHashInvalid
                .code()
                .to_string(),
        ),
        Some(SignedPublishIssueKind::MissingPublisherKeyId)
        | Some(SignedPublishIssueKind::SignatureInvalid)
        | None => SigningError::SignatureInvalid,
    }
}

fn issue_kind_from_error(error: &SigningError) -> SignedPublishIssueKind {
    match error {
        SigningError::SignatureInvalid => SignedPublishIssueKind::SignatureInvalid,
        SigningError::HashError(_) => SignedPublishIssueKind::ManifestHashInvalid,
    }
}

fn sort_signed_publish_issues(issues: &mut [SignedPublishIssue]) {
    issues.sort_by(|a, b| signed_publish_issue_sort_key(a).cmp(&signed_publish_issue_sort_key(b)));
}

fn signed_publish_issue_sort_key(issue: &SignedPublishIssue) -> (&'static str, &str, &str, usize) {
    (
        issue.kind.code(),
        issue.package_name.as_str(),
        issue.package_version.as_str(),
        issue.signer_key.byte_len,
    )
}
