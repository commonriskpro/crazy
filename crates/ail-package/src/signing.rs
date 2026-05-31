// ── ail-package::signing ──────────────────────────────────────────────────
//
// Ed25519 package signing and verification.
//
// # Design decisions
//
// - Signing payload = UTF-8 bytes of `manifest.blake3_hex()` — the manifest's
//   own content hash.  This binds the signature to the full manifest content
//   without re-serialising the manifest inside the signing module.
// - `PackageSignature` stores raw bytes for CBOR determinism.  The 64-byte
//   signature uses a custom `sig_serde` shim (same pattern as `ail-remote`)
//   because `ciborium` does not support fixed-length arrays > 32 natively.
// - `PackageKeypair` wraps `ed25519_dalek::SigningKey` and never exposes the
//   secret key.
//
// # Dependency isolation
//
// This module does NOT depend on `ail-remote`.  The `sig_serde` shim is
// duplicated intentionally — keeping `ail-package` free of upward deps.

use ed25519_dalek::{Signer, Verifier};
use serde::{Deserialize, Serialize};

use crate::manifest::{PackageError, PackageManifest};
use crate::trust::{PackageSigningKeyTrust, PackageSigningTrustPolicy};

// ── sig_serde ─────────────────────────────────────────────────────────────
//
// Custom (de)serialization for `[u8; 64]` via CBOR byte string.

mod sig_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(sig: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(sig)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        let bytes: Vec<u8> = Vec::<u8>::deserialize(d)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("expected exactly 64 bytes for signature"))
    }
}

// ── SigningError ───────────────────────────────────────────────────────────

/// Error returned by package signing and verification operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SigningError {
    /// The signature does not match the package manifest for the given signer.
    SignatureInvalid,
    /// The manifest content hash could not be computed.
    HashError(String),
}

impl std::fmt::Display for SigningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SigningError::SignatureInvalid => write!(f, "Ed25519 package signature is invalid"),
            SigningError::HashError(msg) => write!(f, "manifest hash error: {msg}"),
        }
    }
}

impl std::error::Error for SigningError {}

impl From<PackageError> for SigningError {
    fn from(e: PackageError) -> Self {
        SigningError::HashError(e.0)
    }
}

// ── Package signing diagnostics ───────────────────────────────────────────

/// Stable machine-readable diagnostic kind for package signing and key trust.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PackageSigningDiagnosticKind {
    /// No signature was provided for the manifest.
    MissingSignature,
    /// The manifest could not be hashed for signature verification.
    ManifestHashInvalid,
    /// The Ed25519 signature does not verify for the manifest and signer key.
    SignatureInvalid,
    /// The signer key is not represented in the local trust policy.
    UntrustedKey,
    /// The signer key is represented but expired.
    KeyExpired,
    /// The signer key is represented but revoked.
    KeyRevoked,
}

impl PackageSigningDiagnosticKind {
    /// Stable issue code for downstream audit tooling.
    pub fn code(self) -> &'static str {
        match self {
            PackageSigningDiagnosticKind::MissingSignature => "package.signing.missing_signature",
            PackageSigningDiagnosticKind::ManifestHashInvalid => {
                "package.signing.manifest_hash_invalid"
            }
            PackageSigningDiagnosticKind::SignatureInvalid => "package.signing.signature_invalid",
            PackageSigningDiagnosticKind::UntrustedKey => "package.signing.key_untrusted",
            PackageSigningDiagnosticKind::KeyExpired => "package.signing.key_expired",
            PackageSigningDiagnosticKind::KeyRevoked => "package.signing.key_revoked",
        }
    }

    /// Stable low-cardinality category for production metrics.
    pub fn category(self) -> &'static str {
        match self {
            PackageSigningDiagnosticKind::MissingSignature => "signature_presence",
            PackageSigningDiagnosticKind::ManifestHashInvalid => "manifest_integrity",
            PackageSigningDiagnosticKind::SignatureInvalid => "signature_integrity",
            PackageSigningDiagnosticKind::UntrustedKey
            | PackageSigningDiagnosticKind::KeyExpired
            | PackageSigningDiagnosticKind::KeyRevoked => "key_trust",
        }
    }
}

impl std::fmt::Display for PackageSigningDiagnosticKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

/// Redacted signer key shape for signing/trust diagnostics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSigningKeyShape {
    /// Signature algorithm associated with the key material.
    pub algorithm: String,
    /// Raw public-key byte length, without exposing key bytes.
    pub byte_len: usize,
    /// Always true: diagnostics must not expose raw signing keys.
    pub redacted: bool,
}

impl PackageSigningKeyShape {
    fn from_signature(signature: &PackageSignature) -> Self {
        Self {
            algorithm: "ed25519".to_string(),
            byte_len: signature.signer.len(),
            redacted: true,
        }
    }
}

/// Redacted, stable signing/trust diagnostic suitable for production logs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSigningDiagnostic {
    /// Stable machine-readable diagnostic kind.
    pub kind: PackageSigningDiagnosticKind,
    /// Stable issue code for downstream audit tooling.
    pub code: String,
    /// Stable category for low-cardinality aggregation.
    pub category: String,
    /// Package name associated with the diagnostic.
    pub package_name: String,
    /// Package version associated with the diagnostic.
    pub package_version: String,
    /// Redacted signer key shape, if a signature was present.
    pub signer_key: Option<PackageSigningKeyShape>,
    /// Stable human-readable summary without raw keys or signatures.
    pub message: String,
    /// Always true for diagnostics that deliberately omit sensitive metadata.
    pub redacted: bool,
}

impl PackageSigningDiagnostic {
    fn new(
        kind: PackageSigningDiagnosticKind,
        manifest: &PackageManifest,
        signature: Option<&PackageSignature>,
    ) -> Self {
        Self {
            kind,
            code: kind.code().to_string(),
            category: kind.category().to_string(),
            package_name: manifest.name.clone(),
            package_version: manifest.version.clone(),
            signer_key: signature.map(PackageSigningKeyShape::from_signature),
            message: package_signing_diagnostic_message(kind, manifest),
            redacted: true,
        }
    }
}

/// Redacted signing/trust diagnostic report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSigningDiagnosticReport {
    /// Whether no signing/trust issues were detected.
    pub accepted: bool,
    /// Package name associated with the report.
    pub package_name: String,
    /// Package version associated with the report.
    pub package_version: String,
    /// Deterministically ordered signing/trust issues.
    pub issues: Vec<PackageSigningDiagnostic>,
}

/// Return stable, redacted diagnostics for package signature and key trust.
///
/// This is the diagnostic companion to [`SignedPackage::verify`].  It preserves
/// the existing verification API while giving production callers a structured
/// report that can explain missing signatures, invalid signatures, untrusted
/// keys, and represented expired/revoked keys without logging raw keys or raw
/// signatures.
pub fn diagnose_package_signing_trust(
    manifest: &PackageManifest,
    signature: Option<&PackageSignature>,
    trust_policy: &PackageSigningTrustPolicy,
) -> PackageSigningDiagnosticReport {
    let mut issues = Vec::new();

    let Some(signature) = signature else {
        issues.push(PackageSigningDiagnostic::new(
            PackageSigningDiagnosticKind::MissingSignature,
            manifest,
            None,
        ));
        return package_signing_diagnostic_report(manifest, issues);
    };

    let signed = SignedPackage {
        manifest: manifest.clone(),
        sig: signature.clone(),
    };

    if let Err(error) = signed.verify() {
        issues.push(PackageSigningDiagnostic::new(
            diagnostic_kind_from_signing_error(&error),
            manifest,
            Some(signature),
        ));
    }

    match trust_policy.trust_for_key(&signature.signer) {
        Some(PackageSigningKeyTrust::Trusted) => {}
        Some(PackageSigningKeyTrust::Expired) => issues.push(PackageSigningDiagnostic::new(
            PackageSigningDiagnosticKind::KeyExpired,
            manifest,
            Some(signature),
        )),
        Some(PackageSigningKeyTrust::Revoked) => issues.push(PackageSigningDiagnostic::new(
            PackageSigningDiagnosticKind::KeyRevoked,
            manifest,
            Some(signature),
        )),
        None => issues.push(PackageSigningDiagnostic::new(
            PackageSigningDiagnosticKind::UntrustedKey,
            manifest,
            Some(signature),
        )),
    }

    sort_package_signing_diagnostics(&mut issues);
    package_signing_diagnostic_report(manifest, issues)
}

fn package_signing_diagnostic_report(
    manifest: &PackageManifest,
    issues: Vec<PackageSigningDiagnostic>,
) -> PackageSigningDiagnosticReport {
    PackageSigningDiagnosticReport {
        accepted: issues.is_empty(),
        package_name: manifest.name.clone(),
        package_version: manifest.version.clone(),
        issues,
    }
}

fn diagnostic_kind_from_signing_error(error: &SigningError) -> PackageSigningDiagnosticKind {
    match error {
        SigningError::SignatureInvalid => PackageSigningDiagnosticKind::SignatureInvalid,
        SigningError::HashError(_) => PackageSigningDiagnosticKind::ManifestHashInvalid,
    }
}

fn package_signing_diagnostic_message(
    kind: PackageSigningDiagnosticKind,
    manifest: &PackageManifest,
) -> String {
    let package = &manifest.name;
    let version = &manifest.version;
    match kind {
        PackageSigningDiagnosticKind::MissingSignature => {
            format!("package '{package}' version '{version}' is missing a package signature")
        }
        PackageSigningDiagnosticKind::ManifestHashInvalid => {
            format!("package '{package}' version '{version}' manifest hash could not be computed")
        }
        PackageSigningDiagnosticKind::SignatureInvalid => {
            format!("package '{package}' version '{version}' signature is invalid")
        }
        PackageSigningDiagnosticKind::UntrustedKey => {
            format!("package '{package}' version '{version}' signer key is not trusted")
        }
        PackageSigningDiagnosticKind::KeyExpired => {
            format!("package '{package}' version '{version}' signer key is expired")
        }
        PackageSigningDiagnosticKind::KeyRevoked => {
            format!("package '{package}' version '{version}' signer key is revoked")
        }
    }
}

fn sort_package_signing_diagnostics(issues: &mut [PackageSigningDiagnostic]) {
    issues.sort_by(|a, b| {
        package_signing_diagnostic_sort_key(a).cmp(&package_signing_diagnostic_sort_key(b))
    });
}

fn package_signing_diagnostic_sort_key(
    issue: &PackageSigningDiagnostic,
) -> (PackageSigningDiagnosticKind, &str, &str, usize) {
    (
        issue.kind,
        issue.package_name.as_str(),
        issue.package_version.as_str(),
        issue
            .signer_key
            .as_ref()
            .map(|signer_key| signer_key.byte_len)
            .unwrap_or_default(),
    )
}

// ── PackageSignature ───────────────────────────────────────────────────────

/// An Ed25519 signature over a `PackageManifest` content hash.
///
/// `signer` is the raw 32-byte Ed25519 public key of the entity that signed
/// the manifest.  `signature` is the 64-byte raw Ed25519 signature over
/// `BLAKE3(CBOR(manifest))` expressed as UTF-8 hex bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSignature {
    /// Raw 32-byte Ed25519 public key of the signer.
    pub signer: [u8; 32],
    /// Raw 64-byte Ed25519 signature.
    #[serde(with = "sig_serde")]
    pub signature: [u8; 64],
}

// ── SignedPackage ─────────────────────────────────────────────────────────

/// A `PackageManifest` with an attached Ed25519 signature.
///
/// Use [`SignedPackage::verify`] to confirm the signature is valid before
/// trusting the manifest content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedPackage {
    /// The signed package manifest.
    pub manifest: PackageManifest,
    /// The Ed25519 signature over the manifest content hash.
    pub sig: PackageSignature,
}

impl SignedPackage {
    /// Verify that the attached signature is valid for the embedded manifest.
    ///
    /// The signing payload is the UTF-8 bytes of `manifest.blake3_hex()`.
    ///
    /// # Errors
    ///
    /// - `SigningError::HashError` if `blake3_hex()` fails.
    /// - `SigningError::SignatureInvalid` if the key bytes are malformed or
    ///   the signature does not match.
    pub fn verify(&self) -> Result<(), SigningError> {
        let hash_hex = self.manifest.blake3_hex()?;
        let payload = hash_hex.as_bytes();

        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&self.sig.signer)
            .map_err(|_| SigningError::SignatureInvalid)?;
        let signature = ed25519_dalek::Signature::from_bytes(&self.sig.signature);

        verifying_key
            .verify(payload, &signature)
            .map_err(|_| SigningError::SignatureInvalid)
    }

    /// Return stable, redacted diagnostics for this signed package and trust policy.
    pub fn signing_trust_diagnostics(
        &self,
        trust_policy: &PackageSigningTrustPolicy,
    ) -> PackageSigningDiagnosticReport {
        diagnose_package_signing_trust(&self.manifest, Some(&self.sig), trust_policy)
    }
}

// ── TransparencyLogEntry ──────────────────────────────────────────────────

/// A Sigstore-style transparency log entry for a signed package.
///
/// The transparency log provides a tamper-evident record of every publication
/// event.  Each entry binds a `SignedPackage` to a sequence number, enabling
/// third parties to detect equivocation (two different packages published under
/// the same name/version) and ensure append-only audit trails.
///
/// # Design (docs/packages.md §Open design questions — registry signing model)
///
/// This implements the Sigstore transparency-log concept: every publication
/// records a `log_id` (globally unique), a `sequence` number (monotonically
/// increasing per registry), and the BLAKE3 digest of the signed package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransparencyLogEntry {
    /// Globally unique log entry identifier (e.g., a UUID or registry-assigned ID).
    pub log_id: String,
    /// Monotonically increasing sequence number within this registry.
    pub sequence: u64,
    /// Name of the published package.
    pub package_name: String,
    /// Version of the published package.
    pub package_version: String,
    /// BLAKE3 hex digest of the `SignedPackage` CBOR bytes.
    pub signed_package_hash: String,
    /// Ed25519 public key of the publisher (32 raw bytes, hex-encoded).
    pub publisher_key_hex: String,
}

// ── TransparencyLog ───────────────────────────────────────────────────────

/// An append-only transparency log of signed package publications.
///
/// Maintains a monotonically increasing sequence counter.  Each call to
/// [`TransparencyLog::append`] verifies the `SignedPackage` signature before
/// recording the entry.
#[derive(Clone, Debug, Default)]
pub struct TransparencyLog {
    entries: Vec<TransparencyLogEntry>,
    next_sequence: u64,
}

impl TransparencyLog {
    /// Create a new empty transparency log.
    pub fn new() -> Self {
        TransparencyLog::default()
    }

    /// Append a `SignedPackage` to the log after verifying its signature.
    ///
    /// # Errors
    ///
    /// Returns `SigningError` if signature verification fails or if CBOR
    /// serialization of the signed package fails (for hashing).
    pub fn append(
        &mut self,
        log_id: impl Into<String>,
        signed: &SignedPackage,
    ) -> Result<&TransparencyLogEntry, SigningError> {
        // Verify signature before accepting into the log.
        signed.verify()?;

        // Hash the signed package CBOR bytes.
        let mut cbor_buf = Vec::new();
        ciborium::ser::into_writer(signed, &mut cbor_buf)
            .map_err(|e| SigningError::HashError(format!("CBOR error: {e}")))?;
        let signed_package_hash = {
            let mut hasher = blake3::Hasher::new();
            hasher.update(&cbor_buf);
            hasher.finalize().to_hex().to_string()
        };

        let publisher_key_hex = hex::encode_bytes(&signed.sig.signer);

        let entry = TransparencyLogEntry {
            log_id: log_id.into(),
            sequence: self.next_sequence,
            package_name: signed.manifest.name.clone(),
            package_version: signed.manifest.version.clone(),
            signed_package_hash,
            publisher_key_hex,
        };
        self.next_sequence += 1;
        self.entries.push(entry);
        Ok(self.entries.last().expect("just pushed"))
    }

    /// Return all log entries in sequence order.
    pub fn entries(&self) -> &[TransparencyLogEntry] {
        &self.entries
    }

    /// Return `true` if no entries have been recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Encode bytes as a lower-case hex string.
mod hex {
    pub fn encode_bytes(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

// ── PackageKeypair ────────────────────────────────────────────────────────

/// An Ed25519 signing keypair used to sign `PackageManifest` values.
///
/// The secret key is never exposed via public API.
pub struct PackageKeypair {
    secret: ed25519_dalek::SigningKey,
}

impl PackageKeypair {
    /// Construct a `PackageKeypair` from raw 32-byte secret key bytes.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the bytes do not represent a valid Ed25519 scalar
    /// (all 32-byte inputs are clamped by dalek so this never actually fails,
    /// but the API is kept explicit for forward compatibility).
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self {
            secret: ed25519_dalek::SigningKey::from_bytes(bytes),
        }
    }

    /// Derive the 32-byte public key for this keypair.
    pub fn public_key(&self) -> [u8; 32] {
        self.secret.verifying_key().to_bytes()
    }

    /// Sign a `PackageManifest` and return a `SignedPackage`.
    ///
    /// The signing payload is the UTF-8 bytes of `manifest.blake3_hex()`.
    ///
    /// # Errors
    ///
    /// Returns `SigningError::HashError` if `blake3_hex()` fails.
    pub fn sign_manifest(&self, manifest: PackageManifest) -> Result<SignedPackage, SigningError> {
        let hash_hex = manifest.blake3_hex()?;
        let payload = hash_hex.as_bytes();
        let signature: [u8; 64] = self.secret.sign(payload).to_bytes();
        let signer = self.public_key();
        Ok(SignedPackage {
            manifest,
            sig: PackageSignature { signer, signature },
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{PackageDef, PackageManifest};
    use crate::trust::{PackageSigningTrustAnchor, PackageSigningTrustPolicy, TrustLevel};
    use rand::rngs::OsRng;

    fn minimal_manifest() -> PackageManifest {
        PackageManifest::from_def(PackageDef {
            name: "test.pkg".to_string(),
            version: "1.0.0".to_string(),
            trust_level: TrustLevel::Verified,
            required_capabilities: vec![],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
            handlers: vec![],
            contracts: vec![],
            exports: vec![],
            imports: vec![],
            boundaries: vec![],
            license: None,
            provenance: None,
            verification_report: None,
            graph_schema: None,
            core_ir_schema: None,
            // 4G fields
            reproducible_evidence: None,
        })
    }

    fn generate_keypair() -> PackageKeypair {
        let secret = ed25519_dalek::SigningKey::generate(&mut OsRng);
        PackageKeypair { secret }
    }

    fn signing_trust_policy_for(
        public_key: [u8; 32],
        trust_anchor: fn([u8; 32]) -> PackageSigningTrustAnchor,
    ) -> PackageSigningTrustPolicy {
        PackageSigningTrustPolicy::from_trust_anchors(vec![trust_anchor(public_key)])
    }

    fn lower_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    // ── signing_diagnostics_report_missing_signature ─────────────────────
    // Production gate: unsigned package input gets a stable redacted issue.
    #[test]
    fn signing_diagnostics_report_missing_signature() {
        let manifest = minimal_manifest();
        let policy = PackageSigningTrustPolicy::new();

        let report = diagnose_package_signing_trust(&manifest, None, &policy);

        assert!(!report.accepted);
        assert_eq!(report.package_name, "test.pkg");
        assert_eq!(report.package_version, "1.0.0");
        assert_eq!(report.issues.len(), 1);
        let issue = &report.issues[0];
        assert_eq!(issue.kind, PackageSigningDiagnosticKind::MissingSignature);
        assert_eq!(issue.code, "package.signing.missing_signature");
        assert_eq!(issue.category, "signature_presence");
        assert_eq!(issue.signer_key, None);
        assert!(issue.redacted);
    }

    // ── signing_diagnostics_report_invalid_signature ─────────────────────
    // Production gate: invalid signatures get stable diagnostics without raw
    // signature material.
    #[test]
    fn signing_diagnostics_report_invalid_signature() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let mut signed = kp.sign_manifest(manifest).expect("sign must succeed");
        signed.manifest.version = "9.9.9".to_string();
        let policy = signing_trust_policy_for(kp.public_key(), PackageSigningTrustAnchor::trusted);

        let report = signed.signing_trust_diagnostics(&policy);

        assert!(!report.accepted);
        assert_eq!(report.issues.len(), 1);
        let issue = &report.issues[0];
        assert_eq!(issue.kind, PackageSigningDiagnosticKind::SignatureInvalid);
        assert_eq!(issue.code, "package.signing.signature_invalid");
        assert_eq!(issue.category, "signature_integrity");
        assert_eq!(
            issue.signer_key,
            Some(PackageSigningKeyShape {
                algorithm: "ed25519".to_string(),
                byte_len: 32,
                redacted: true,
            })
        );

        let rendered = format!("{report:?}");
        assert!(
            !rendered.contains(&lower_hex(&signed.sig.signature)),
            "diagnostics must not leak raw signature bytes: {rendered}"
        );
    }

    // ── signing_diagnostics_report_untrusted_key ─────────────────────────
    // Production gate: valid signatures from keys outside the local trust
    // policy get key-trust diagnostics.
    #[test]
    fn signing_diagnostics_report_untrusted_key() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let signed = kp.sign_manifest(manifest).expect("sign must succeed");
        let policy = PackageSigningTrustPolicy::new();

        let report = signed.signing_trust_diagnostics(&policy);

        assert!(!report.accepted);
        assert_eq!(report.issues.len(), 1);
        let issue = &report.issues[0];
        assert_eq!(issue.kind, PackageSigningDiagnosticKind::UntrustedKey);
        assert_eq!(issue.code, "package.signing.key_untrusted");
        assert_eq!(issue.category, "key_trust");

        let rendered = format!("{report:?}");
        assert!(
            !rendered.contains(&lower_hex(&signed.sig.signer)),
            "diagnostics must not leak raw signer keys: {rendered}"
        );
    }

    // ── signing_diagnostics_report_expired_key ───────────────────────────
    // Production gate: represented expired keys are distinct from unknown keys.
    #[test]
    fn signing_diagnostics_report_expired_key() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let signed = kp.sign_manifest(manifest).expect("sign must succeed");
        let policy = signing_trust_policy_for(kp.public_key(), PackageSigningTrustAnchor::expired);

        let report = signed.signing_trust_diagnostics(&policy);

        assert!(!report.accepted);
        assert_eq!(report.issues.len(), 1);
        assert_eq!(
            report.issues[0].kind,
            PackageSigningDiagnosticKind::KeyExpired
        );
        assert_eq!(report.issues[0].code, "package.signing.key_expired");
    }

    // ── signing_diagnostics_report_revoked_key ───────────────────────────
    // Production gate: represented revoked keys are distinct from unknown keys.
    #[test]
    fn signing_diagnostics_report_revoked_key() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let signed = kp.sign_manifest(manifest).expect("sign must succeed");
        let policy = signing_trust_policy_for(kp.public_key(), PackageSigningTrustAnchor::revoked);

        let report = signed.signing_trust_diagnostics(&policy);

        assert!(!report.accepted);
        assert_eq!(report.issues.len(), 1);
        assert_eq!(
            report.issues[0].kind,
            PackageSigningDiagnosticKind::KeyRevoked
        );
        assert_eq!(report.issues[0].code, "package.signing.key_revoked");
    }

    // ── signing_diagnostics_order_is_deterministic ───────────────────────
    // Production gate: multi-issue reports are sorted by stable issue kind.
    #[test]
    fn signing_diagnostics_order_is_deterministic() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let mut signed = kp.sign_manifest(manifest).expect("sign must succeed");
        signed.manifest.version = "9.9.9".to_string();
        let policy = PackageSigningTrustPolicy::new();

        let report = signed.signing_trust_diagnostics(&policy);

        let kinds = report
            .issues
            .iter()
            .map(|issue| issue.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                PackageSigningDiagnosticKind::SignatureInvalid,
                PackageSigningDiagnosticKind::UntrustedKey,
            ]
        );
    }

    // ── signing_diagnostics_accept_trusted_valid_signature ───────────────
    // Control: a valid signature with a trusted key emits no diagnostics.
    #[test]
    fn signing_diagnostics_accept_trusted_valid_signature() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let signed = kp.sign_manifest(manifest).expect("sign must succeed");
        let policy = signing_trust_policy_for(kp.public_key(), PackageSigningTrustAnchor::trusted);

        let report = signed.signing_trust_diagnostics(&policy);

        assert!(report.accepted);
        assert!(report.issues.is_empty());
    }

    // ── RED: package_signature_cbor_round_trip ────────────────────────────
    // Spec: REQ-SIGN-5 — SignedPackage is CBOR-serializable/deserializable
    //   GIVEN a SignedPackage with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn signed_package_cbor_round_trip() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let signed = kp.sign_manifest(manifest).expect("sign must succeed");

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&signed, &mut buf).expect("CBOR serialize must succeed");
        let decoded: SignedPackage =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialize must succeed");

        assert_eq!(
            decoded, signed,
            "round-tripped SignedPackage must equal original"
        );
    }

    // ── RED: sign_verify_roundtrip ────────────────────────────────────────
    // Spec: REQ-SIGN-3, REQ-SIGN-4
    //   GIVEN a PackageKeypair and a PackageManifest
    //   WHEN sign_manifest is called and then verify is called on the result
    //   THEN verify returns Ok(())
    #[test]
    fn sign_verify_roundtrip() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let signed = kp.sign_manifest(manifest).expect("sign must succeed");
        signed
            .verify()
            .expect("valid signature must verify successfully");
    }

    // ── RED: wrong_key_rejects_signature ─────────────────────────────────
    // Spec: REQ-SIGN-4 — wrong signer key returns SignatureInvalid
    //   GIVEN a SignedPackage signed by keypair A
    //   WHEN the signer field is replaced with keypair B's public key
    //   THEN verify returns Err(SigningError::SignatureInvalid)
    #[test]
    fn wrong_key_rejects_signature() {
        let kp_a = generate_keypair();
        let kp_b = generate_keypair();
        let manifest = minimal_manifest();
        let mut signed = kp_a.sign_manifest(manifest).expect("sign must succeed");
        // Tamper: replace signer with a different public key
        signed.sig.signer = kp_b.public_key();
        assert_eq!(
            signed.verify(),
            Err(SigningError::SignatureInvalid),
            "wrong signer key must return SignatureInvalid"
        );
    }

    // ── RED: tampered_manifest_rejects_signature ──────────────────────────
    // TRIANGULATE: if the manifest changes after signing, verify rejects it
    //   GIVEN a SignedPackage with an original manifest
    //   WHEN manifest.version is changed after signing
    //   THEN verify returns Err(SigningError::SignatureInvalid)
    #[test]
    fn tampered_manifest_rejects_signature() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let mut signed = kp.sign_manifest(manifest).expect("sign must succeed");
        // Tamper: modify manifest content
        signed.manifest.version = "9.9.9".to_string();
        assert_eq!(
            signed.verify(),
            Err(SigningError::SignatureInvalid),
            "tampered manifest must not verify"
        );
    }

    // ── RED: public_key_roundtrip ─────────────────────────────────────────
    // Spec: REQ-SIGN-1 — signer public key embedded in signature matches keypair
    //   GIVEN a PackageKeypair
    //   WHEN sign_manifest is called
    //   THEN signed.sig.signer == kp.public_key()
    #[test]
    fn public_key_embedded_in_signature() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let signed = kp.sign_manifest(manifest).expect("sign must succeed");
        assert_eq!(
            signed.sig.signer,
            kp.public_key(),
            "embedded signer must equal keypair public key"
        );
    }

    // ── transparency_log_append_verified_entry ────────────────────────────
    // Spec scenario: "Transparency log records a verified publication"
    //   GIVEN a valid SignedPackage
    //   WHEN append() is called on a TransparencyLog
    //   THEN the entry is recorded with correct sequence, name, and version
    #[test]
    fn transparency_log_append_verified_entry() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let signed = kp.sign_manifest(manifest).expect("sign");

        let mut log = TransparencyLog::new();
        assert!(log.is_empty());

        let entry = log
            .append("entry-001", &signed)
            .expect("append must succeed");
        assert_eq!(entry.sequence, 0);
        assert_eq!(entry.package_name, "test.pkg");
        assert_eq!(entry.package_version, "1.0.0");
        assert_eq!(entry.log_id, "entry-001");
        assert_eq!(entry.signed_package_hash.len(), 64);
        assert_eq!(log.len(), 1);
    }

    // ── transparency_log_sequence_is_monotonic ────────────────────────────
    // TRIANGULATE: sequence numbers are monotonically increasing
    #[test]
    fn transparency_log_sequence_is_monotonic() {
        let kp = generate_keypair();
        let m1 = minimal_manifest();
        let m2 = {
            let mut def = crate::manifest::PackageDef {
                name: "test.pkg".to_string(),
                version: "2.0.0".to_string(),
                trust_level: crate::trust::TrustLevel::Verified,
                required_capabilities: vec![],
                exported_capabilities: vec![],
                assumptions: vec![],
                unsafe_surface: vec![],
                artifact_hashes: vec![],
                build_env_hash: None,
                handlers: vec![],
                contracts: vec![],
                exports: vec![],
                imports: vec![],
                boundaries: vec![],
                license: None,
                provenance: None,
                verification_report: None,
                graph_schema: None,
                core_ir_schema: None,
                // 4G fields
                reproducible_evidence: None,
            };
            def.version = "2.0.0".to_string();
            crate::manifest::PackageManifest::from_def(def)
        };
        let s1 = kp.sign_manifest(m1).expect("sign");
        let s2 = kp.sign_manifest(m2).expect("sign");

        let mut log = TransparencyLog::new();
        let e1 = log.append("a", &s1).expect("append 1");
        assert_eq!(e1.sequence, 0);
        let e2 = log.append("b", &s2).expect("append 2");
        assert_eq!(e2.sequence, 1);
        assert_eq!(log.len(), 2);
    }

    // ── transparency_log_rejects_tampered_package ─────────────────────────
    // Spec scenario: "Transparency log rejects packages with invalid signatures"
    //   GIVEN a SignedPackage with a tampered manifest
    //   WHEN append() is called
    //   THEN returns Err(SigningError::SignatureInvalid)
    #[test]
    fn transparency_log_rejects_tampered_package() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let mut signed = kp.sign_manifest(manifest).expect("sign");
        signed.manifest.version = "9.9.9".to_string(); // tamper

        let mut log = TransparencyLog::new();
        let result = log.append("tampered", &signed);
        assert!(
            matches!(result, Err(SigningError::SignatureInvalid)),
            "tampered package must be rejected by transparency log"
        );
        assert!(log.is_empty(), "failed append must not add entry");
    }

    // ── transparency_log_entry_cbor_round_trip ────────────────────────────
    // TRIANGULATE: TransparencyLogEntry is CBOR-serializable
    #[test]
    fn transparency_log_entry_cbor_round_trip() {
        use super::TransparencyLogEntry;
        let entry = TransparencyLogEntry {
            log_id: "log-001".to_string(),
            sequence: 42,
            package_name: "payments.stripe".to_string(),
            package_version: "1.2.0".to_string(),
            signed_package_hash: "a".repeat(64),
            publisher_key_hex: "b".repeat(64),
        };
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&entry, &mut buf).expect("encode");
        let decoded: TransparencyLogEntry =
            ciborium::de::from_reader(buf.as_slice()).expect("decode");
        assert_eq!(decoded, entry);
    }
}
