// ── ail-package ───────────────────────────────────────────────────────────
//
// Package manifest, trust, and registry types for the AIL package model.
//
// # Dependency isolation rules
//
// This crate depends only on:
//   - `ail-core`      (graph primitives, no policy)
//   - `blake3`        (hashing)
//   - `ciborium`      (CBOR serialization)
//   - `ed25519-dalek` (signing)
//   - `serde`         (derive macros)
//   - `serde_json`    (JSON serialization for HTTP registry transport)
//
// It MUST NOT depend on `ail-verify`, `ail-runtime`, or `ail-compiler`.
// The dependency graph is:
//   `ail-package` → `ail-core`
//   `ail-verify`  → `ail-package`
//   `ail-runtime` → `ail-package`
//
// Introducing an upward dependency would create a cycle.

pub mod advisory;
pub mod assumption;
pub mod coherence;
pub mod export;
pub mod generated_artifact;
pub mod handler;
pub mod http_registry;
pub mod import;
pub mod lockfile;
pub mod manifest;
pub mod namespace;
pub mod policy;
pub mod registry;
pub mod remote_registry;
pub mod resolver;
pub mod signing;
pub mod surface;
pub mod trust;
pub mod verification;
pub mod versioning;
pub mod yank;

// ── Public re-exports ─────────────────────────────────────────────────────

pub use advisory::{AdvisoryChecker, AdvisorySeverity, SecurityAdvisory};
pub use assumption::{
    ApprovalRecord, AssumptionEnforcementError, AssumptionEnforcer, AssumptionState,
    PackageAssumption,
};
pub use coherence::{CoherenceChecker, CoherenceError, InterfaceImpl};
pub use export::{ExportDeclaration, ExportStability, ExportVisibility};
pub use generated_artifact::GeneratedArtifact;
pub use handler::HandlerExport;
pub use http_registry::{HttpClientError, HttpRegistryClient, HttpRegistryServer};
pub use import::ImportDeclaration;
pub use lockfile::{Lockfile, LockfileEntry};
pub use manifest::{
    ArtifactHashEntry, PackageDef, PackageError, PackageManifest, PackageValidationError,
    Provenance, ReproducibleBuildEvidence,
};
pub use namespace::{
    ImportAlias, NamespaceKind, NamespaceOwnershipCheck, OwnershipError, PackageNamespace,
};
pub use policy::{
    CapabilityPolicy, CapabilityPolicyEnforcer, CapabilityPolicyVerdict, CapabilityViolation,
    DeploymentProfile, TrustGate, TrustGateVerdict,
};
pub use registry::PackageRegistry;
pub use remote_registry::{
    FetchRequest, FetchResponse, InMemoryError, InMemoryRegistryClient, PublishRequest,
    PublishResponse, RegistryClient, RemoteRegistryDiagnostic, RemoteRegistryDiagnosticKind,
    RemoteRegistryDiagnosticRedaction, RemoteRegistryDiagnosticReport, RemoteRegistryOperation,
    SearchRequest, SearchResponse, SearchResult, VerifyOutcome, VerifyRequest, VerifyResponse,
    publish_signed,
};
pub use resolver::{DependencyResolver, DependencySpec, ResolverError};
pub use signing::{PackageKeypair, PackageSignature, SignedPackage, SigningError};
pub use surface::UnsafeSurfaceEntry;
pub use trust::TrustLevel;
pub use verification::{
    PackageVerificationEvidenceError, PackageVerificationReport, validate_verified_package_evidence,
};
pub use versioning::{
    CompatibilityClass, CompatibilityEngine, CompatibilityError, LocalCompatibilityIssue,
    LocalCompatibilityIssueKind, MigrationRecord, MigrationStep, PackageCompatibilityMetadata,
    PackageVersioning, VersionRequirement, VersionRequirementError, VersionRequirementIssue,
    VersionRequirementIssueCode, VersionRequirementIssueKind, VersionRequirementShape,
};
pub use yank::YankRecord;
