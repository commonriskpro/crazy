//! # ail-runtime
//!
//! Wasmtime-backed capability host for the AI-native language runtime.
//!
//! # What this crate does
//! - Owns the only direct `wasmtime` dependency in the workspace.
//! - Enforces a deny-by-default capability policy via preflight checks.
//! - Validates WASM module hashes and capability manifests before instantiation.
//! - Appends exactly one redacted [`AuditEvent`] per preflight call.
//!
//! # What this crate does NOT do (Phase 8 PR 2)
//! - No host-call linker registration (handler wiring deferred to a later phase).
//! - No CLI surface or Context Server integration.
//! - No persistent audit backend (events are in-memory only).
//!
//! # Architecture
//!
//! ```text
//! wasm bytes + CapabilityManifest + RuntimeProfile
//!         │
//!         ▼
//! RuntimeHost::validate_and_instantiate
//!         │ hash wasm, hash manifest
//!         │ check required capabilities ⊆ grants
//!         │ validate Module with Wasmtime
//!         │ append PreflightPassed / PreflightFailed
//!         ▼
//! RuntimeInstance (only on pass)
//! ```

pub mod abi;
pub mod audit;
pub mod error;
pub mod host;
pub mod manifest;
pub mod profile;

pub use ail_package::manifest::PackageManifest;
pub use ail_package::trust::TrustLevel;

pub use abi::{HostCallId, HostError, HostResult};
pub use audit::{AuditEvent, AuditLog};
pub use error::{PreflightFailure, RuntimeError, RuntimeResult};
pub use host::{RuntimeHost, RuntimeInstance};
pub use manifest::{CapabilityManifest, blake3_hex_of};
pub use profile::{CapabilityGrant, CapabilityId, ResourceLimits, RuntimeProfile};
