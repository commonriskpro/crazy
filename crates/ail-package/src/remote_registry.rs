// ── ail-package::remote_registry ─────────────────────────────────────────
//
// Remote registry client/server protocol types for the AIL package registry.
//
// # Design (docs/packages.md §Open design questions — Package registry protocol)
//
// This module defines the request/response protocol for the remote package
// registry operations:
//   - publish: submit a signed package to the registry
//   - fetch:   retrieve a signed package by name/version
//   - search:  list packages matching a query
//   - verify:  check package integrity and advisory status
//
// The protocol is message-oriented.  Messages are CBOR-serializable for
// transport over any byte stream.  Authentication uses Ed25519 signatures
// (via `SignedPackage`).
//
// # Dependency isolation
//
// This module does NOT import a network runtime.  Callers wire the transport.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;

use crate::advisory::{AdvisorySeverity, SecurityAdvisory};
use crate::manifest::PackageManifest;
use crate::signing::SignedPackage;

// ── Publish ───────────────────────────────────────────────────────────────

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

// ── RegistryClient (trait) ────────────────────────────────────────────────

/// Trait defining the remote registry client interface.
///
/// Implementors provide the transport (HTTP, gRPC, in-process mock, etc.).
/// This trait is synchronous and returns owned values; async wrappers can be
/// layered on top.
pub trait RegistryClient {
    /// Error type returned by all registry operations.
    type Error: std::fmt::Debug;

    /// Publish a signed package to the registry.
    fn publish(&self, request: PublishRequest) -> Result<PublishResponse, Self::Error>;

    /// Fetch a specific package version from the registry.
    fn fetch(&self, request: FetchRequest) -> Result<FetchResponse, Self::Error>;

    /// Search for packages matching a query.
    fn search(&self, request: SearchRequest) -> Result<SearchResponse, Self::Error>;

    /// Verify a package's integrity and advisory status.
    fn verify(&self, request: VerifyRequest) -> Result<VerifyResponse, Self::Error>;
}

// ── InMemoryRegistryClient ────────────────────────────────────────────────

/// A fully in-memory `RegistryClient` implementation for testing.
///
/// Uses `PackageRegistry` and in-memory advisory/yank state.
pub struct InMemoryRegistryClient {
    registry: crate::registry::PackageRegistry,
    signed_packages: RefCell<Vec<SignedPackage>>,
    advisories: Vec<SecurityAdvisory>,
    yank_records: Vec<crate::yank::YankRecord>,
}

impl InMemoryRegistryClient {
    /// Create a new empty in-memory registry client.
    pub fn new() -> Self {
        InMemoryRegistryClient {
            registry: crate::registry::PackageRegistry::new(),
            signed_packages: RefCell::new(Vec::new()),
            advisories: Vec::new(),
            yank_records: Vec::new(),
        }
    }

    /// Add an advisory to the in-memory store.
    pub fn add_advisory(&mut self, advisory: SecurityAdvisory) {
        self.advisories.push(advisory);
    }
}

impl Default for InMemoryRegistryClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Error type for `InMemoryRegistryClient`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InMemoryError {
    /// Signature verification failed during publish.
    SignatureInvalid,
    /// CBOR serialization failed.
    SerializationError(String),
}

impl std::fmt::Display for InMemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InMemoryError::SignatureInvalid => write!(f, "invalid package signature"),
            InMemoryError::SerializationError(msg) => write!(f, "serialization error: {msg}"),
        }
    }
}

impl std::error::Error for InMemoryError {}

impl RegistryClient for InMemoryRegistryClient {
    type Error = InMemoryError;

    fn publish(&self, request: PublishRequest) -> Result<PublishResponse, Self::Error> {
        // Verify the signature.
        if request.signed_package.verify().is_err() {
            return Ok(PublishResponse {
                accepted: false,
                error: Some("signature verification failed".to_string()),
                log_id: None,
                sequence: None,
            });
        }
        let mut signed_packages = self.signed_packages.borrow_mut();
        signed_packages.retain(|signed| {
            signed.manifest.name != request.signed_package.manifest.name
                || signed.manifest.version != request.signed_package.manifest.version
        });
        let sequence = signed_packages.len() as u64;
        signed_packages.push(request.signed_package);
        Ok(PublishResponse {
            accepted: true,
            error: None,
            log_id: Some(format!("mem-log-{sequence:03}")),
            sequence: Some(sequence),
        })
    }

    fn fetch(&self, request: FetchRequest) -> Result<FetchResponse, Self::Error> {
        if let Some(signed) = self
            .signed_packages
            .borrow()
            .iter()
            .find(|signed| {
                signed.manifest.name == request.name && signed.manifest.version == request.version
            })
            .cloned()
        {
            let yanked = self.registry.is_yanked(&request.name, &request.version);
            return Ok(FetchResponse {
                signed_package: Some(signed),
                yanked,
                error: None,
            });
        }

        let manifest = self
            .registry
            .lookup_by_name_version(&request.name, &request.version);

        match manifest {
            None => Ok(FetchResponse {
                signed_package: None,
                yanked: false,
                error: Some(format!(
                    "package {} {} not found",
                    request.name, request.version
                )),
            }),
            Some(m) => {
                let yanked = self.registry.is_yanked(&request.name, &request.version);
                let _ = m;
                Ok(FetchResponse {
                    signed_package: None,
                    yanked,
                    error: None,
                })
            }
        }
    }

    fn search(&self, request: SearchRequest) -> Result<SearchResponse, Self::Error> {
        let results: Vec<SearchResult> = self
            .registry
            .all()
            .iter()
            .filter(|m| {
                m.name
                    .to_lowercase()
                    .contains(&request.query.to_lowercase())
            })
            .take(request.limit.unwrap_or(20) as usize)
            .map(|m| SearchResult {
                name: m.name.clone(),
                latest_version: m.version.clone(),
                description: None,
            })
            .collect();

        let total_matching = self
            .registry
            .all()
            .iter()
            .filter(|m| {
                m.name
                    .to_lowercase()
                    .contains(&request.query.to_lowercase())
            })
            .count();

        Ok(SearchResponse {
            truncated: total_matching > results.len(),
            results,
        })
    }

    fn verify(&self, request: VerifyRequest) -> Result<VerifyResponse, Self::Error> {
        // Check yanked first.
        if self
            .yank_records
            .iter()
            .any(|y| y.name == request.name && y.version == request.version)
        {
            let reason = self
                .yank_records
                .iter()
                .find(|y| y.name == request.name && y.version == request.version)
                .map(|y| y.reason.clone())
                .unwrap_or_default();
            return Ok(VerifyResponse {
                outcome: VerifyOutcome::Yanked { reason },
            });
        }

        // Check advisory.
        if let Some(adv) = crate::advisory::AdvisoryChecker::first_match(
            &request.name,
            &request.version,
            &self.advisories,
        ) {
            return Ok(VerifyResponse {
                outcome: VerifyOutcome::Advisory {
                    advisory_id: adv.id.clone(),
                    severity: adv.severity,
                },
            });
        }

        // Lookup manifest.
        match self
            .registry
            .lookup_by_name_version(&request.name, &request.version)
        {
            None => Ok(VerifyResponse {
                outcome: VerifyOutcome::NotFound,
            }),
            Some(m) => {
                // Compare hash.
                match m.blake3_hex() {
                    Ok(hash) if hash == request.expected_hash => Ok(VerifyResponse {
                        outcome: VerifyOutcome::Ok,
                    }),
                    Ok(hash) => Ok(VerifyResponse {
                        outcome: VerifyOutcome::HashMismatch {
                            registry_hash: hash,
                        },
                    }),
                    Err(e) => Err(InMemoryError::SerializationError(e.0)),
                }
            }
        }
    }
}

// ── Extend PackageRegistry for signed publish ─────────────────────────────

/// Extension that registers a `SignedPackage` into a `PackageRegistry` after
/// verifying its signature.
///
/// This wires signing into the publish workflow: callers cannot bypass
/// signature verification when publishing through this API.
pub fn publish_signed(
    registry: &mut crate::registry::PackageRegistry,
    signed: &SignedPackage,
) -> Result<PackageManifest, crate::signing::SigningError> {
    signed.verify()?;
    let manifest = signed.manifest.clone();
    registry.register(manifest.clone());
    Ok(manifest)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisory::{AdvisorySeverity, SecurityAdvisory};
    use crate::manifest::{PackageDef, PackageManifest};
    use crate::registry::PackageRegistry;
    use crate::signing::PackageKeypair;
    use crate::trust::TrustLevel;
    use rand::rngs::OsRng;

    fn make_manifest(name: &str, version: &str) -> PackageManifest {
        PackageManifest::from_def(PackageDef {
            name: name.to_string(),
            version: version.to_string(),
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
        })
    }

    fn gen_keypair() -> PackageKeypair {
        let secret = ed25519_dalek::SigningKey::generate(&mut OsRng);
        PackageKeypair::from_bytes(&secret.to_bytes())
    }

    // ── publish_request_cbor_round_trip ───────────────────────────────────
    // Spec scenario: "PublishRequest round-trips through CBOR"
    #[test]
    fn publish_request_cbor_round_trip() {
        let kp = gen_keypair();
        let manifest = make_manifest("payments.stripe", "1.2.0");
        let signed = kp.sign_manifest(manifest).expect("sign");
        let req = PublishRequest {
            signed_package: signed,
        };
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&req, &mut buf).expect("encode");
        let decoded: PublishRequest = ciborium::de::from_reader(buf.as_slice()).expect("decode");
        assert_eq!(decoded, req);
    }

    // ── in_memory_client_publish_accepts_valid ────────────────────────────
    // Spec scenario: "In-memory client accepts a valid signed package"
    #[test]
    fn in_memory_client_publish_accepts_valid() {
        let kp = gen_keypair();
        let manifest = make_manifest("payments.stripe", "1.2.0");
        let signed = kp.sign_manifest(manifest).expect("sign");
        let client = InMemoryRegistryClient::new();
        let resp = client
            .publish(PublishRequest {
                signed_package: signed,
            })
            .expect("no transport error");
        assert!(resp.accepted);
        assert!(resp.error.is_none());
    }

    // ── in_memory_client_publish_persists_for_fetch ──────────────────────
    // Spec scenario: "publish + fetch round-trip returns the signed package"
    #[test]
    fn in_memory_client_publish_persists_for_fetch() {
        let kp = gen_keypair();
        let manifest = make_manifest("payments.stripe", "1.2.0");
        let signed = kp.sign_manifest(manifest).expect("sign");
        let client = InMemoryRegistryClient::new();

        let publish = client
            .publish(PublishRequest {
                signed_package: signed.clone(),
            })
            .expect("publish");
        assert!(publish.accepted);

        let fetch = client
            .fetch(FetchRequest {
                name: "payments.stripe".to_string(),
                version: "1.2.0".to_string(),
            })
            .expect("fetch");

        assert_eq!(fetch.signed_package, Some(signed));
        assert!(!fetch.yanked);
        assert!(fetch.error.is_none());
    }

    // ── in_memory_client_publish_rejects_tampered ─────────────────────────
    // Spec scenario: "In-memory client rejects tampered packages"
    #[test]
    fn in_memory_client_publish_rejects_tampered() {
        let kp = gen_keypair();
        let manifest = make_manifest("payments.stripe", "1.2.0");
        let mut signed = kp.sign_manifest(manifest).expect("sign");
        signed.manifest.version = "9.9.9".to_string(); // tamper
        let client = InMemoryRegistryClient::new();
        let resp = client
            .publish(PublishRequest {
                signed_package: signed,
            })
            .expect("no transport error");
        assert!(!resp.accepted);
        assert!(resp.error.is_some());
    }

    // ── in_memory_client_search ───────────────────────────────────────────
    // Spec scenario: "Search returns packages matching query"
    #[test]
    fn in_memory_client_search() {
        let mut client = InMemoryRegistryClient::new();
        client
            .registry
            .register(make_manifest("payments.stripe", "1.2.0"));
        client
            .registry
            .register(make_manifest("payments.paypal", "2.0.0"));
        client
            .registry
            .register(make_manifest("utils.core", "0.1.0"));

        let resp = client
            .search(SearchRequest {
                query: "payments".to_string(),
                limit: None,
            })
            .expect("no transport error");
        assert_eq!(resp.results.len(), 2);
        assert!(!resp.truncated);
    }

    // ── in_memory_client_verify_ok ────────────────────────────────────────
    // Spec scenario: "Verify returns Ok for matching hash"
    #[test]
    fn in_memory_client_verify_ok() {
        let mut client = InMemoryRegistryClient::new();
        let manifest = make_manifest("payments.stripe", "1.2.0");
        let expected_hash = manifest.blake3_hex().expect("hash");
        client.registry.register(manifest);

        let resp = client
            .verify(VerifyRequest {
                name: "payments.stripe".to_string(),
                version: "1.2.0".to_string(),
                expected_hash,
            })
            .expect("no transport error");
        assert_eq!(resp.outcome, VerifyOutcome::Ok);
    }

    // ── in_memory_client_verify_hash_mismatch ─────────────────────────────
    // Spec scenario: "Verify returns HashMismatch for wrong hash"
    #[test]
    fn in_memory_client_verify_hash_mismatch() {
        let mut client = InMemoryRegistryClient::new();
        client
            .registry
            .register(make_manifest("payments.stripe", "1.2.0"));

        let resp = client
            .verify(VerifyRequest {
                name: "payments.stripe".to_string(),
                version: "1.2.0".to_string(),
                expected_hash: "z".repeat(64),
            })
            .expect("no transport error");
        assert!(matches!(resp.outcome, VerifyOutcome::HashMismatch { .. }));
    }

    // ── in_memory_client_verify_not_found ─────────────────────────────────
    // Spec scenario: "Verify returns NotFound for absent package"
    #[test]
    fn in_memory_client_verify_not_found() {
        let client = InMemoryRegistryClient::new();
        let resp = client
            .verify(VerifyRequest {
                name: "unknown.pkg".to_string(),
                version: "1.0.0".to_string(),
                expected_hash: "a".repeat(64),
            })
            .expect("no transport error");
        assert_eq!(resp.outcome, VerifyOutcome::NotFound);
    }

    // ── in_memory_client_verify_advisory ─────────────────────────────────
    // Spec scenario: "Verify returns Advisory for affected package"
    #[test]
    fn in_memory_client_verify_advisory() {
        let mut client = InMemoryRegistryClient::new();
        client
            .registry
            .register(make_manifest("payments.stripe", "1.0.0"));
        client.add_advisory(SecurityAdvisory {
            id: "adv_001".to_string(),
            package: "payments.stripe".to_string(),
            affected_constraint: "1.0.0".to_string(),
            severity: AdvisorySeverity::Critical,
            reason: "bug".to_string(),
        });

        let resp = client
            .verify(VerifyRequest {
                name: "payments.stripe".to_string(),
                version: "1.0.0".to_string(),
                expected_hash: "a".repeat(64),
            })
            .expect("no transport error");
        assert!(
            matches!(resp.outcome, VerifyOutcome::Advisory { advisory_id, .. } if advisory_id == "adv_001")
        );
    }

    // ── publish_signed_wires_signing_into_registry ────────────────────────
    // Spec scenario: "publish_signed verifies signature before registering"
    #[test]
    fn publish_signed_wires_signing_into_registry() {
        let kp = gen_keypair();
        let manifest = make_manifest("payments.stripe", "1.2.0");
        let signed = kp.sign_manifest(manifest.clone()).expect("sign");

        let mut registry = PackageRegistry::new();
        let result = publish_signed(&mut registry, &signed);
        assert!(result.is_ok());
        assert!(
            registry
                .lookup_by_name_version("payments.stripe", "1.2.0")
                .is_some()
        );
    }

    // ── publish_signed_rejects_tampered ───────────────────────────────────
    // TRIANGULATE: tampered package is rejected
    #[test]
    fn publish_signed_rejects_tampered() {
        let kp = gen_keypair();
        let manifest = make_manifest("payments.stripe", "1.2.0");
        let mut signed = kp.sign_manifest(manifest).expect("sign");
        signed.manifest.version = "9.9.9".to_string(); // tamper

        let mut registry = PackageRegistry::new();
        assert!(publish_signed(&mut registry, &signed).is_err());
        assert!(registry.is_empty());
    }

    // ── fetch_response_cbor_round_trip ────────────────────────────────────
    // TRIANGULATE: FetchResponse with no signed_package round-trips through CBOR
    #[test]
    fn fetch_response_cbor_round_trip() {
        let resp = FetchResponse {
            signed_package: None,
            yanked: false,
            error: Some("not found".to_string()),
        };
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&resp, &mut buf).expect("encode");
        let decoded: FetchResponse = ciborium::de::from_reader(buf.as_slice()).expect("decode");
        assert_eq!(decoded, resp);
    }
}
