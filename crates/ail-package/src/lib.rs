// ── ail-package ───────────────────────────────────────────────────────────
//
// Package manifest, trust, and registry types for the AIL package model.
//
// # Dependency isolation rules
//
// This crate depends only on:
//   - `ail-core`  (graph primitives, no policy)
//   - `blake3`    (hashing)
//   - `ciborium`  (CBOR serialization)
//   - `serde`     (derive macros)
//
// It MUST NOT depend on `ail-verify`, `ail-runtime`, or `ail-compiler`.
// The dependency graph is:
//   `ail-package` → `ail-core`
//   `ail-verify`  → `ail-package`
//   `ail-runtime` → `ail-package`
//
// Introducing an upward dependency would create a cycle.

pub mod assumption;
pub mod export;
pub mod handler;
pub mod import;
pub mod lockfile;
pub mod manifest;
pub mod registry;
pub mod surface;
pub mod trust;
pub mod verification;

// ── Public re-exports ─────────────────────────────────────────────────────

pub use assumption::{AssumptionState, PackageAssumption};
pub use export::{ExportDeclaration, ExportStability, ExportVisibility};
pub use handler::HandlerExport;
pub use import::ImportDeclaration;
pub use lockfile::LockfileEntry;
pub use manifest::{
    ArtifactHashEntry, PackageDef, PackageError, PackageManifest, PackageValidationError,
};
pub use registry::PackageRegistry;
pub use surface::UnsafeSurfaceEntry;
pub use trust::TrustLevel;
pub use verification::PackageVerificationReport;
