// ── ail-runtime::host_dispatch::result_diagnostics ────────────────────────
//
// Stable redacted diagnostics for WASM-side host dispatch results.
//
// These diagnostics are additive: dispatch return values and audit behavior
// stay unchanged, while production callers can inspect deterministic issue
// descriptors without exposing raw capability names, handler names, payloads,
// or host error messages.

use crate::abi::HostError;
use crate::profile::CapabilityId;

/// Stable category for a WASM host-dispatch result diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HostDispatchResultDiagnosticKind {
    /// Dispatch arguments could not be decoded from the WASM boundary.
    MalformedArgs,
    /// The capability was granted, but no handler was bound.
    HandlerMissing,
    /// A bound handler returned an error.
    HostError,
    /// The handler response did not satisfy the expected dispatch result ABI.
    ResultAbiMismatch,
}

/// One redacted deterministic host-dispatch result diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostDispatchResultDiagnostic {
    /// Stable issue kind for grouping.
    pub kind: HostDispatchResultDiagnosticKind,
    /// Deterministic key suitable for sorting, deduplication, and dashboards.
    pub diagnostic_key: String,
    /// Redacted capability/operation/handler shape, never raw labels.
    pub subject: String,
    /// Stable classifier within the diagnostic kind.
    pub classification: String,
    /// Redacted operational detail.
    pub detail: String,
}

impl HostDispatchResultDiagnostic {
    fn new(
        kind: HostDispatchResultDiagnosticKind,
        subject: String,
        classification: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        let classification = classification.into();
        let detail = detail.into();
        let diagnostic_key = format!(
            "host_dispatch_result/{kind:?}/{subject}/{classification}",
            kind = kind,
            subject = subject,
            classification = classification,
        );
        Self {
            kind,
            diagnostic_key,
            subject,
            classification,
            detail,
        }
    }

    pub(crate) fn malformed_args(
        capability: Option<&CapabilityId>,
        operation: Option<&str>,
        classification: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self::new(
            HostDispatchResultDiagnosticKind::MalformedArgs,
            call_subject(capability, operation, None),
            classification,
            detail,
        )
    }

    pub(crate) fn handler_missing(capability: &CapabilityId, operation: &str) -> Self {
        Self::new(
            HostDispatchResultDiagnosticKind::HandlerMissing,
            call_subject(Some(capability), Some(operation), None),
            "handler.missing",
            "no handler bound for granted capability",
        )
    }

    pub(crate) fn host_error(
        capability: &CapabilityId,
        operation: &str,
        handler_name: &str,
        error: &HostError,
    ) -> Self {
        let classification = host_error_classification(error);
        Self::new(
            HostDispatchResultDiagnosticKind::HostError,
            call_subject(Some(capability), Some(operation), Some(handler_name)),
            classification,
            format!("error_shape={classification}"),
        )
    }

    pub(crate) fn result_abi_mismatch(
        capability: &CapabilityId,
        operation: &str,
        handler_name: &str,
        classification: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self::new(
            HostDispatchResultDiagnosticKind::ResultAbiMismatch,
            call_subject(Some(capability), Some(operation), Some(handler_name)),
            classification,
            detail,
        )
    }
}

/// Sort and deduplicate diagnostics deterministically by stable key.
pub fn sort_host_dispatch_result_diagnostics(
    mut diagnostics: Vec<HostDispatchResultDiagnostic>,
) -> Vec<HostDispatchResultDiagnostic> {
    diagnostics.sort_by(|left, right| left.diagnostic_key.cmp(&right.diagnostic_key));
    diagnostics.dedup_by(|left, right| left.diagnostic_key == right.diagnostic_key);
    diagnostics
}

fn call_subject(
    capability: Option<&CapabilityId>,
    operation: Option<&str>,
    handler_name: Option<&str>,
) -> String {
    format!(
        "capability={},operation={},handler={}",
        capability
            .map(|cap| redacted_label(cap.as_str()))
            .unwrap_or_else(|| "unknown".to_string()),
        operation
            .map(redacted_label)
            .unwrap_or_else(|| "unknown".to_string()),
        handler_name
            .map(redacted_label)
            .unwrap_or_else(|| "unknown".to_string()),
    )
}

fn redacted_label(value: &str) -> String {
    let hash = blake3::hash(value.as_bytes()).to_hex().to_string();
    format!("h{}:len{}", &hash[..12], value.len())
}

fn host_error_classification(error: &HostError) -> &'static str {
    match error {
        HostError::CapabilityDenied(_) => "host_error.capability_denied",
        HostError::CapabilityDeniedCategorized { .. } => "host_error.capability_denied",
        HostError::HandlerNotBound(_) => "host_error.handler_not_bound",
        HostError::PayloadDecodeError(_) => "host_error.payload_decode",
        HostError::PayloadEncodeError(_) => "host_error.payload_encode",
        HostError::ContractViolation(_) => "host_error.contract_violation",
        HostError::Timeout(_) => "host_error.timeout",
        HostError::LimitExceeded(_) => "host_error.limit_exceeded",
        HostError::HandlerUnavailable(_) => "host_error.handler_unavailable",
        HostError::BoundaryFailure(_) => "host_error.boundary_failure",
        HostError::AuditFailure(_) => "host_error.audit_failure",
        HostError::ManifestMismatch(_) => "host_error.manifest_mismatch",
        HostError::Custom(_) => "host_error.custom",
    }
}
