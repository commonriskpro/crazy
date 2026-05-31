// ── ail-runtime::manifest ────────────────────────────────────────────────
//
// `CapabilityManifest` — declares which capabilities a WASM module requires.
//
// The manifest hash is computed as:
//   `blake3(canonical_cbor(manifest))`
//
// where `canonical_cbor` produces a deterministic byte sequence via
// `ciborium`.  Preflight compares this hash against the
// `capability_manifest_hash` recorded in `RuntimeProfile`.

use blake3::Hasher;
use ciborium::ser::into_writer;
use serde::{Deserialize, Serialize};

use crate::profile::{CapabilityId, RateLimit, ResourceLimits, RuntimeProfile};

// ── CapabilityManifest ───────────────────────────────────────────────────

/// Declares the capability requirements of one WASM module.
///
/// The manifest is provided by the module author (or build toolchain) and
/// is validated against the profile's grants at preflight time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CapabilityManifest {
    /// Name of the WASM module this manifest describes.
    pub module: String,

    /// Capabilities required by the module, in declaration order.
    ///
    /// Preflight checks that every entry here has a matching
    /// [`CapabilityGrant`](crate::profile::CapabilityGrant) in the profile.
    pub requires: Vec<CapabilityId>,
}

impl CapabilityManifest {
    /// Compute the BLAKE3 manifest hash as a hex-encoded string.
    ///
    /// The hash covers the canonical CBOR serialization of the manifest.
    /// Returns an error string if CBOR serialization fails.
    pub fn blake3_hex(&self) -> Result<String, String> {
        let mut buf = Vec::new();
        into_writer(self, &mut buf).map_err(|e| format!("CBOR serialization failed: {e}"))?;
        let hash = blake3_hex_of(&buf);
        Ok(hash)
    }
}

// We need `CapabilityId` to be CBOR-serializable for the manifest hash.
impl Serialize for CapabilityId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

// ── RuntimeArtifactManifest diagnostics ──────────────────────────────────

/// Current runtime-facing artifact manifest schema identifier.
pub const RUNTIME_ARTIFACT_MANIFEST_SCHEMA_VERSION: &str = "runtime-artifact-manifest/1.0";

/// Stable diagnostic key for stale runtime artifact manifest schemas.
pub const RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_STALE_SCHEMA: &str =
    "runtime.artifact_manifest.stale_schema";
/// Stable diagnostic key for missing runtime artifact manifest module names.
pub const RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_MISSING_MODULE: &str =
    "runtime.artifact_manifest.missing_module";
/// Stable diagnostic key for missing runtime artifact manifest hashes.
pub const RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_MISSING_HASH: &str =
    "runtime.artifact_manifest.missing_hash";
/// Stable diagnostic key for missing runtime artifact manifest profiles.
pub const RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_MISSING_PROFILE: &str =
    "runtime.artifact_manifest.missing_profile";
/// Stable diagnostic key for runtime artifact profile mismatches.
pub const RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_PROFILE_MISMATCH: &str =
    "runtime.artifact_manifest.profile_mismatch";
/// Stable diagnostic key for runtime artifact hash mismatches.
pub const RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_HASH_MISMATCH: &str =
    "runtime.artifact_manifest.hash_mismatch";
/// Stable diagnostic key for runtime artifact limit mismatches.
pub const RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_LIMIT_MISMATCH: &str =
    "runtime.artifact_manifest.limit_mismatch";

/// Runtime-facing deployment manifest for one artifact.
///
/// This mirrors the runtime-critical subset needed before production startup:
/// schema freshness, bound module/profile identity, sealed artifact hashes, and
/// resource limits. Fields are optional so legacy or partial manifests can be
/// diagnosed without failing deserialization at the boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeArtifactManifest {
    /// Runtime artifact manifest schema identifier.
    pub schema_version: Option<String>,
    /// Module name the artifact was produced for.
    pub module: Option<String>,
    /// Runtime profile name the artifact was produced for.
    pub profile: Option<String>,
    /// BLAKE3 hex digest of the expected WASM module bytes.
    pub module_hash: Option<String>,
    /// BLAKE3 hex digest of the canonical capability manifest sidecar.
    pub capability_manifest_hash: Option<String>,
    /// Resource limits sealed into the artifact manifest.
    #[serde(default)]
    pub limits: RuntimeArtifactLimits,
}

impl RuntimeArtifactManifest {
    /// Build a runtime artifact manifest from the runtime profile and manifest.
    pub fn from_profile(profile: &RuntimeProfile, manifest: &CapabilityManifest) -> Self {
        Self {
            schema_version: Some(RUNTIME_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string()),
            module: Some(manifest.module.clone()),
            profile: Some(profile.name().to_string()),
            module_hash: Some(profile.module_hash().to_string()),
            capability_manifest_hash: Some(profile.capability_manifest_hash().to_string()),
            limits: RuntimeArtifactLimits::from_resource_limits(profile.limits()),
        }
    }

    /// Return stable redacted diagnostics against the runtime profile.
    ///
    /// Diagnostics are canonical and de-duplicated. Raw profile names, module
    /// names, hash values, and limit values are never emitted.
    pub fn diagnostics_for_profile(
        &self,
        profile: &RuntimeProfile,
    ) -> Vec<RuntimeArtifactManifestDiagnostic> {
        let mut diagnostics = Vec::new();

        if self.schema_version.as_deref() != Some(RUNTIME_ARTIFACT_MANIFEST_SCHEMA_VERSION) {
            diagnostics.push(RuntimeArtifactManifestDiagnostic::new(
                RuntimeArtifactManifestDiagnosticKind::StaleSchema,
                "schema_version",
                expected_schema_descriptor(),
                presence_descriptor(self.schema_version.as_deref(), "schema"),
            ));
        }

        match self.module.as_deref() {
            Some(module) if !module.is_empty() => {}
            _ => diagnostics.push(RuntimeArtifactManifestDiagnostic::new(
                RuntimeArtifactManifestDiagnosticKind::MissingModule,
                "module",
                "module:<present>",
                "module:<missing>",
            )),
        }

        match self.profile.as_deref() {
            Some(artifact_profile) if artifact_profile == profile.name() => {}
            Some(_) => diagnostics.push(RuntimeArtifactManifestDiagnostic::new(
                RuntimeArtifactManifestDiagnosticKind::ProfileMismatch,
                "profile",
                "profile:<runtime>",
                "profile:<artifact>",
            )),
            None => diagnostics.push(RuntimeArtifactManifestDiagnostic::new(
                RuntimeArtifactManifestDiagnosticKind::MissingProfile,
                "profile",
                "profile:<runtime>",
                "profile:<missing>",
            )),
        }

        diagnose_hash_field(
            &mut diagnostics,
            "module_hash",
            self.module_hash.as_deref(),
            profile.module_hash(),
        );
        diagnose_hash_field(
            &mut diagnostics,
            "capability_manifest_hash",
            self.capability_manifest_hash.as_deref(),
            profile.capability_manifest_hash(),
        );

        for mismatch in self.limits.mismatches(profile.limits()) {
            diagnostics.push(RuntimeArtifactManifestDiagnostic::new(
                RuntimeArtifactManifestDiagnosticKind::LimitMismatch,
                mismatch,
                "limit:<runtime>",
                "limit:<artifact>",
            ));
        }

        sort_and_dedup_runtime_artifact_manifest_diagnostics(diagnostics)
    }
}

/// Runtime artifact view of resource limits.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeArtifactLimits {
    /// Maximum linear memory in bytes, if constrained.
    pub max_memory_bytes: Option<u64>,
    /// Maximum Wasmtime fuel units, if constrained.
    pub max_fuel: Option<u64>,
    /// Maximum wall-clock execution time in milliseconds.
    pub timeout_millis: Option<u64>,
    /// Maximum number of capability calls per invocation.
    pub max_capability_calls: Option<u64>,
    /// Per-capability or global rate limits.
    pub rate_limits: Option<Vec<RuntimeArtifactRateLimit>>,
    /// Maximum payload size in bytes for a single capability call.
    pub payload_size_limit: Option<u64>,
    /// Maximum number of concurrent in-flight capability calls.
    pub concurrency_limit: Option<u64>,
    /// Maximum recursion depth / WASM call-stack frames.
    pub recursion_stack_limit: Option<u64>,
    /// Maximum size of a capability response payload in bytes.
    pub output_size_limit: Option<u64>,
}

impl RuntimeArtifactLimits {
    /// Build a runtime artifact limit snapshot from resource limits.
    pub fn from_resource_limits(limits: &ResourceLimits) -> Self {
        Self {
            max_memory_bytes: limits.max_memory_bytes,
            max_fuel: limits.max_fuel,
            timeout_millis: limits.timeout.map(duration_millis_saturating),
            max_capability_calls: limits.max_capability_calls,
            rate_limits: limits.rate_limits.as_ref().map(|rate_limits| {
                rate_limits
                    .iter()
                    .map(RuntimeArtifactRateLimit::from_rate_limit)
                    .collect()
            }),
            payload_size_limit: limits.payload_size_limit,
            concurrency_limit: limits.concurrency_limit,
            recursion_stack_limit: limits.recursion_stack_limit,
            output_size_limit: limits.output_size_limit,
        }
    }

    fn mismatches(&self, limits: &ResourceLimits) -> Vec<&'static str> {
        let expected = RuntimeArtifactLimits::from_resource_limits(limits);
        let mut mismatches = Vec::new();

        push_if_mismatch(
            &mut mismatches,
            "limits.max_memory_bytes",
            &self.max_memory_bytes,
            &expected.max_memory_bytes,
        );
        push_if_mismatch(
            &mut mismatches,
            "limits.max_fuel",
            &self.max_fuel,
            &expected.max_fuel,
        );
        push_if_mismatch(
            &mut mismatches,
            "limits.timeout_millis",
            &self.timeout_millis,
            &expected.timeout_millis,
        );
        push_if_mismatch(
            &mut mismatches,
            "limits.max_capability_calls",
            &self.max_capability_calls,
            &expected.max_capability_calls,
        );
        if canonical_runtime_artifact_rate_limits(self.rate_limits.as_deref())
            != canonical_runtime_artifact_rate_limits(expected.rate_limits.as_deref())
        {
            mismatches.push("limits.rate_limits");
        }
        push_if_mismatch(
            &mut mismatches,
            "limits.payload_size_limit",
            &self.payload_size_limit,
            &expected.payload_size_limit,
        );
        push_if_mismatch(
            &mut mismatches,
            "limits.concurrency_limit",
            &self.concurrency_limit,
            &expected.concurrency_limit,
        );
        push_if_mismatch(
            &mut mismatches,
            "limits.recursion_stack_limit",
            &self.recursion_stack_limit,
            &expected.recursion_stack_limit,
        );
        push_if_mismatch(
            &mut mismatches,
            "limits.output_size_limit",
            &self.output_size_limit,
            &expected.output_size_limit,
        );

        mismatches
    }
}

/// Runtime artifact view of a per-capability or global rate limit.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RuntimeArtifactRateLimit {
    /// Capability this limit applies to, or `None` for global limits.
    pub capability: Option<String>,
    /// Maximum calls per second allowed.
    pub max_calls_per_second: u64,
}

impl RuntimeArtifactRateLimit {
    /// Build a runtime artifact rate-limit snapshot from a runtime rate limit.
    pub fn from_rate_limit(limit: &RateLimit) -> Self {
        Self {
            capability: limit.capability.clone(),
            max_calls_per_second: limit.max_calls_per_second,
        }
    }
}

/// Production-safe runtime artifact manifest diagnostic kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RuntimeArtifactManifestDiagnosticKind {
    /// The manifest schema is absent or not current.
    StaleSchema,
    /// The manifest does not identify its module.
    MissingModule,
    /// The manifest is missing a required artifact hash.
    MissingHash,
    /// The manifest does not identify its runtime profile.
    MissingProfile,
    /// The manifest was produced for a different runtime profile.
    ProfileMismatch,
    /// A manifest hash does not match the runtime profile seal.
    HashMismatch,
    /// A manifest limit does not match the runtime profile limit.
    LimitMismatch,
}

impl RuntimeArtifactManifestDiagnosticKind {
    /// Stable machine-readable key for this diagnostic kind.
    pub const fn diagnostic_key(self) -> &'static str {
        match self {
            RuntimeArtifactManifestDiagnosticKind::StaleSchema => {
                RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_STALE_SCHEMA
            }
            RuntimeArtifactManifestDiagnosticKind::MissingModule => {
                RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_MISSING_MODULE
            }
            RuntimeArtifactManifestDiagnosticKind::MissingHash => {
                RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_MISSING_HASH
            }
            RuntimeArtifactManifestDiagnosticKind::MissingProfile => {
                RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_MISSING_PROFILE
            }
            RuntimeArtifactManifestDiagnosticKind::ProfileMismatch => {
                RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_PROFILE_MISMATCH
            }
            RuntimeArtifactManifestDiagnosticKind::HashMismatch => {
                RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_HASH_MISMATCH
            }
            RuntimeArtifactManifestDiagnosticKind::LimitMismatch => {
                RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_LIMIT_MISMATCH
            }
        }
    }
}

/// Stable redacted descriptor for one runtime artifact manifest issue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RuntimeArtifactManifestDiagnostic {
    /// Canonical diagnostic kind.
    pub kind: RuntimeArtifactManifestDiagnosticKind,
    /// Stable machine-readable key for dashboards/support triage.
    pub diagnostic_key: &'static str,
    /// Manifest/profile field that failed validation.
    pub field: &'static str,
    /// Redacted expected value descriptor.
    pub expected: &'static str,
    /// Redacted actual value descriptor.
    pub actual: &'static str,
}

impl RuntimeArtifactManifestDiagnostic {
    fn new(
        kind: RuntimeArtifactManifestDiagnosticKind,
        field: &'static str,
        expected: &'static str,
        actual: &'static str,
    ) -> Self {
        Self {
            kind,
            diagnostic_key: kind.diagnostic_key(),
            field,
            expected,
            actual,
        }
    }

    fn sort_key(
        &self,
    ) -> (
        RuntimeArtifactManifestDiagnosticKind,
        &'static str,
        &'static str,
        &'static str,
    ) {
        (self.kind, self.field, self.expected, self.actual)
    }
}

fn diagnose_hash_field(
    diagnostics: &mut Vec<RuntimeArtifactManifestDiagnostic>,
    field: &'static str,
    actual: Option<&str>,
    expected: &str,
) {
    match actual {
        Some(actual) if actual == expected => {}
        Some(_) => diagnostics.push(RuntimeArtifactManifestDiagnostic::new(
            RuntimeArtifactManifestDiagnosticKind::HashMismatch,
            field,
            "hash:<runtime>",
            "hash:<artifact>",
        )),
        None => diagnostics.push(RuntimeArtifactManifestDiagnostic::new(
            RuntimeArtifactManifestDiagnosticKind::MissingHash,
            field,
            "hash:<runtime>",
            "hash:<missing>",
        )),
    }
}

fn push_if_mismatch<T: Eq>(
    mismatches: &mut Vec<&'static str>,
    field: &'static str,
    actual: &T,
    expected: &T,
) {
    if actual != expected {
        mismatches.push(field);
    }
}

fn duration_millis_saturating(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn canonical_runtime_artifact_rate_limits(
    rate_limits: Option<&[RuntimeArtifactRateLimit]>,
) -> Vec<RuntimeArtifactRateLimit> {
    let mut rate_limits = rate_limits.unwrap_or_default().to_vec();
    rate_limits.sort();
    rate_limits.dedup();
    rate_limits
}

fn expected_schema_descriptor() -> &'static str {
    "schema:<current>"
}

fn presence_descriptor(value: Option<&str>, label: &'static str) -> &'static str {
    if value.is_some_and(|value| !value.is_empty()) {
        match label {
            "schema" => "schema:<artifact>",
            _ => "value:<present>",
        }
    } else {
        match label {
            "schema" => "schema:<missing>",
            _ => "value:<missing>",
        }
    }
}

fn sort_and_dedup_runtime_artifact_manifest_diagnostics(
    mut diagnostics: Vec<RuntimeArtifactManifestDiagnostic>,
) -> Vec<RuntimeArtifactManifestDiagnostic> {
    diagnostics.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
    diagnostics.dedup_by(|left, right| left.sort_key() == right.sort_key());
    diagnostics
}

// ── Internal helpers ──────────────────────────────────────────────────────

/// Compute a BLAKE3 hex digest of a byte slice.
///
/// Exposed publicly so callers can pre-compute the `module_hash` to store in
/// a [`RuntimeProfile`](crate::profile::RuntimeProfile) before preflight.
pub fn blake3_hex_of(bytes: &[u8]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(bytes);
    hasher.finalize().to_hex().to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Structural: manifest is constructible and fields are accessible.
    #[test]
    fn manifest_fields_are_accessible() {
        let id = CapabilityId::new("FileRead");
        let m = CapabilityManifest {
            module: "test-module".to_string(),
            requires: vec![id.clone()],
        };
        assert_eq!(m.module, "test-module");
        assert_eq!(m.requires.len(), 1);
        assert_eq!(m.requires[0], id);
    }

    // blake3_hex produces a 64-character hex string (256-bit hash).
    #[test]
    fn blake3_hex_returns_64_char_string() {
        let m = CapabilityManifest {
            module: "m".to_string(),
            requires: vec![],
        };
        let hex = m.blake3_hex().expect("serialization must succeed");
        assert_eq!(hex.len(), 64, "BLAKE3 hex must be 64 characters");
    }

    // TRIANGULATE: same manifest → same hash (determinism).
    #[test]
    fn blake3_hex_is_deterministic() {
        let m = CapabilityManifest {
            module: "m".to_string(),
            requires: vec![CapabilityId::new("X")],
        };
        let h1 = m.blake3_hex().unwrap();
        let h2 = m.blake3_hex().unwrap();
        assert_eq!(h1, h2, "hash must be deterministic");
    }

    // TRIANGULATE: different manifests → different hashes.
    #[test]
    fn blake3_hex_differs_for_different_manifests() {
        let m1 = CapabilityManifest {
            module: "m1".to_string(),
            requires: vec![],
        };
        let m2 = CapabilityManifest {
            module: "m2".to_string(),
            requires: vec![CapabilityId::new("FileRead")],
        };
        assert_ne!(
            m1.blake3_hex().unwrap(),
            m2.blake3_hex().unwrap(),
            "different manifests must have different hashes"
        );
    }

    fn runtime_profile_with_limits(limits: ResourceLimits) -> RuntimeProfile {
        let manifest = CapabilityManifest {
            module: "checkout.private.module".to_string(),
            requires: vec![CapabilityId::new("secret.read:customer-token")],
        };

        RuntimeProfile::new(
            "prod-critical-private".to_string(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            manifest.blake3_hex().expect("manifest hash must succeed"),
            vec![],
            limits,
        )
    }

    fn complete_runtime_artifact_manifest(
        profile: &RuntimeProfile,
        limits: RuntimeArtifactLimits,
    ) -> RuntimeArtifactManifest {
        RuntimeArtifactManifest {
            schema_version: Some(RUNTIME_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string()),
            module: Some("checkout.private.module".to_string()),
            profile: Some(profile.name().to_string()),
            module_hash: Some(profile.module_hash().to_string()),
            capability_manifest_hash: Some(profile.capability_manifest_hash().to_string()),
            limits,
        }
    }

    #[test]
    fn runtime_artifact_manifest_reports_stale_schema_and_missing_identity_hashes() {
        let profile = runtime_profile_with_limits(ResourceLimits::default());
        let artifact = RuntimeArtifactManifest {
            schema_version: Some("runtime-artifact-manifest/0.9".to_string()),
            module: None,
            profile: None,
            module_hash: None,
            capability_manifest_hash: None,
            limits: RuntimeArtifactLimits::from_resource_limits(profile.limits()),
        };

        let diagnostics = artifact.diagnostics_for_profile(&profile);
        let keys: Vec<(&str, &str, &str)> = diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.diagnostic_key,
                    diagnostic.field,
                    diagnostic.actual,
                )
            })
            .collect();

        assert_eq!(
            keys,
            vec![
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_STALE_SCHEMA,
                    "schema_version",
                    "schema:<artifact>",
                ),
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_MISSING_MODULE,
                    "module",
                    "module:<missing>",
                ),
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_MISSING_HASH,
                    "capability_manifest_hash",
                    "hash:<missing>",
                ),
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_MISSING_HASH,
                    "module_hash",
                    "hash:<missing>",
                ),
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_MISSING_PROFILE,
                    "profile",
                    "profile:<missing>",
                ),
            ]
        );
    }

    #[test]
    fn runtime_artifact_manifest_reports_profile_hash_and_limit_mismatches_redacted() {
        let limits = ResourceLimits {
            max_memory_bytes: Some(4096),
            max_fuel: Some(30),
            timeout: Some(std::time::Duration::from_millis(250)),
            max_capability_calls: Some(3),
            rate_limits: Some(vec![RateLimit {
                capability: Some("secret.read:customer-token".to_string()),
                max_calls_per_second: 2,
            }]),
            payload_size_limit: Some(512),
            concurrency_limit: Some(1),
            recursion_stack_limit: Some(8),
            output_size_limit: Some(1024),
        };
        let profile = runtime_profile_with_limits(limits);
        let mut artifact = complete_runtime_artifact_manifest(
            &profile,
            RuntimeArtifactLimits::from_resource_limits(profile.limits()),
        );
        artifact.profile = Some("dev-private".to_string());
        artifact.module_hash =
            Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string());
        artifact.capability_manifest_hash =
            Some("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string());
        artifact.limits.max_memory_bytes = Some(2048);
        artifact.limits.rate_limits = None;

        let diagnostics = artifact.diagnostics_for_profile(&profile);
        let triples: Vec<(&str, &str, &str)> = diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.diagnostic_key,
                    diagnostic.field,
                    diagnostic.actual,
                )
            })
            .collect();

        assert_eq!(
            triples,
            vec![
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_PROFILE_MISMATCH,
                    "profile",
                    "profile:<artifact>",
                ),
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_HASH_MISMATCH,
                    "capability_manifest_hash",
                    "hash:<artifact>",
                ),
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_HASH_MISMATCH,
                    "module_hash",
                    "hash:<artifact>",
                ),
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_LIMIT_MISMATCH,
                    "limits.max_memory_bytes",
                    "limit:<artifact>",
                ),
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_LIMIT_MISMATCH,
                    "limits.rate_limits",
                    "limit:<artifact>",
                ),
            ]
        );

        let rendered = format!("{diagnostics:?}");
        assert!(!rendered.contains("prod-critical-private"));
        assert!(!rendered.contains("dev-private"));
        assert!(!rendered.contains("checkout.private.module"));
        assert!(!rendered.contains("customer-token"));
        assert!(!rendered.contains("aaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn runtime_artifact_manifest_limit_rate_limits_are_order_insensitive() {
        let limits = ResourceLimits {
            rate_limits: Some(vec![
                RateLimit {
                    capability: Some("secret.read:alpha".to_string()),
                    max_calls_per_second: 1,
                },
                RateLimit {
                    capability: None,
                    max_calls_per_second: 9,
                },
            ]),
            ..ResourceLimits::default()
        };
        let profile = runtime_profile_with_limits(limits);
        let mut artifact = complete_runtime_artifact_manifest(
            &profile,
            RuntimeArtifactLimits::from_resource_limits(profile.limits()),
        );
        artifact.limits.rate_limits = Some(vec![
            RuntimeArtifactRateLimit {
                capability: None,
                max_calls_per_second: 9,
            },
            RuntimeArtifactRateLimit {
                capability: Some("secret.read:alpha".to_string()),
                max_calls_per_second: 1,
            },
        ]);

        assert_eq!(artifact.diagnostics_for_profile(&profile), vec![]);
    }

    #[test]
    fn runtime_artifact_manifest_diagnostics_sort_and_dedup_deterministically() {
        let duplicate = RuntimeArtifactManifestDiagnostic::new(
            RuntimeArtifactManifestDiagnosticKind::MissingHash,
            "module_hash",
            "hash:<runtime>",
            "hash:<missing>",
        );
        let diagnostics = sort_and_dedup_runtime_artifact_manifest_diagnostics(vec![
            duplicate.clone(),
            RuntimeArtifactManifestDiagnostic::new(
                RuntimeArtifactManifestDiagnosticKind::StaleSchema,
                "schema_version",
                "schema:<current>",
                "schema:<artifact>",
            ),
            duplicate,
        ]);

        let keys: Vec<(&str, &str)> = diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.diagnostic_key, diagnostic.field))
            .collect();

        assert_eq!(
            keys,
            vec![
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_STALE_SCHEMA,
                    "schema_version",
                ),
                (
                    RUNTIME_ARTIFACT_MANIFEST_DIAGNOSTIC_KEY_MISSING_HASH,
                    "module_hash",
                ),
            ]
        );
    }
}
