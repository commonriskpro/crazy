use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use crate::advisory::SecurityAdvisory;
use crate::manifest::PackageManifest;
use crate::signing::SignedPackage;

use super::{
    FetchRequest, FetchResponse, PublishRequest, PublishResponse, RegistryClient, SearchRequest,
    SearchResponse, SearchResult, VerifyOutcome, VerifyRequest, VerifyResponse,
};

/// A fully in-memory `RegistryClient` implementation for testing.
///
/// Uses `PackageRegistry` and in-memory advisory/yank state.
///
/// ## Sequence monotonicity
///
/// Each accepted publish increments a dedicated counter regardless of whether
/// the same name/version was previously published.  Re-publishing the same
/// package/version replaces the stored entry but the sequence number still
/// advances, preserving the transparency-log invariant that sequence numbers
/// are strictly increasing across the lifetime of a registry instance.
///
/// ## Single-thread contract
///
/// `signed_packages` and `next_sequence` use `RefCell`/`Cell` for interior
/// mutability.  Both types are `!Sync` — the compiler prevents sharing across
/// threads.  The HTTP server creates one `InMemoryRegistryClient` per spawned
/// thread and processes connections serially, so no synchronisation is
/// required.  A future multi-threaded registry would need to wrap both fields
/// together in a `Mutex<(Vec<SignedPackage>, u64)>` to keep the two mutations
/// atomic as a pair.
pub struct InMemoryRegistryClient {
    pub(super) registry: crate::registry::PackageRegistry,
    signed_packages: RefCell<Vec<SignedPackage>>,
    advisories: Vec<SecurityAdvisory>,
    yank_records: Vec<crate::yank::YankRecord>,
    /// Monotonically increasing counter for transparency-log sequence numbers.
    ///
    /// Incremented on every accepted publish.  Never derived from collection
    /// length so it remains monotonic even when `retain` shrinks the store.
    next_sequence: Cell<u64>,
}

impl InMemoryRegistryClient {
    /// Create a new empty in-memory registry client.
    pub fn new() -> Self {
        InMemoryRegistryClient {
            registry: crate::registry::PackageRegistry::new(),
            signed_packages: RefCell::new(Vec::new()),
            advisories: Vec::new(),
            yank_records: Vec::new(),
            next_sequence: Cell::new(0),
        }
    }

    /// Add an advisory to the in-memory store.
    pub fn add_advisory(&mut self, advisory: SecurityAdvisory) {
        self.advisories.push(advisory);
    }

    /// Record a yank in the in-memory registry metadata.
    pub fn yank(
        &mut self,
        name: impl Into<String>,
        version: impl Into<String>,
        reason: impl Into<String>,
    ) {
        let name = name.into();
        let version = version.into();
        let reason = reason.into();
        self.registry
            .yank(name.clone(), version.clone(), reason.clone());
        self.yank_records.push(crate::yank::YankRecord {
            name,
            version,
            reason,
        });
    }

    fn yank_reason(&self, name: &str, version: &str) -> Option<String> {
        self.yank_records
            .iter()
            .find(|y| y.name == name && y.version == version)
            .map(|y| y.reason.clone())
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
        // Use the dedicated monotonic counter — never derived from collection
        // length so re-publishing the same name/version still advances the
        // sequence rather than repeating the previous value.
        let sequence = self.next_sequence.get();
        self.next_sequence.set(sequence + 1);
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
        let query = request.query.to_lowercase();
        let limit = request.limit.unwrap_or(20) as usize;
        let mut by_name: BTreeMap<String, PackageManifest> = BTreeMap::new();

        for m in self.registry.all() {
            if m.name.to_lowercase().contains(&query) {
                insert_latest(&mut by_name, m.clone());
            }
        }

        for signed in self.signed_packages.borrow().iter() {
            let m = &signed.manifest;
            if m.name.to_lowercase().contains(&query) {
                insert_latest(&mut by_name, m.clone());
            }
        }

        let matches: Vec<_> = by_name
            .into_values()
            .map(|m| SearchResult {
                name: m.name,
                latest_version: m.version,
                description: None,
            })
            .collect();

        let truncated = matches.len() > limit;
        let results = matches.into_iter().take(limit).collect();

        Ok(SearchResponse { truncated, results })
    }

    fn verify(&self, request: VerifyRequest) -> Result<VerifyResponse, Self::Error> {
        // Check yanked first.
        if let Some(reason) = self.yank_reason(&request.name, &request.version) {
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

        // Lookup manifest. Signed publications are registry metadata too; the
        // fallback preserves direct in-memory test fixture registration.
        let published_manifest = self
            .signed_packages
            .borrow()
            .iter()
            .find(|signed| {
                signed.manifest.name == request.name && signed.manifest.version == request.version
            })
            .map(|signed| signed.manifest.clone());
        let fixture_manifest = self
            .registry
            .lookup_by_name_version(&request.name, &request.version)
            .cloned();

        match published_manifest.or(fixture_manifest) {
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

fn insert_latest(by_name: &mut BTreeMap<String, PackageManifest>, manifest: PackageManifest) {
    match by_name.get(&manifest.name) {
        Some(existing) if !version_is_newer(&manifest.version, &existing.version) => {}
        _ => {
            by_name.insert(manifest.name.clone(), manifest);
        }
    }
}

fn version_is_newer(candidate: &str, existing: &str) -> bool {
    match (
        semver::Version::parse(candidate),
        semver::Version::parse(existing),
    ) {
        (Ok(candidate), Ok(existing)) => candidate > existing,
        _ => candidate > existing,
    }
}
