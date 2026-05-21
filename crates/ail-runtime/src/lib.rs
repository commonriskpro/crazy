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
//! # What this crate does NOT do
//! - No CLI surface or Context Server integration.
//! - No persistent audit backend (events are in-memory only).
//! - Wasmtime Linker host-import wiring deferred to PR2.
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
//!         │ [opt] check handler binding
//!         │ append PreflightPassed / PreflightFailed
//!         ▼
//! RuntimeInstance (only on pass)
//!
//! RuntimeHost::call_capability
//!         │ check grant → HandlerDispatch → Handler::handle
//!         │ append CapabilityCallExecuted
//!         ▼
//! HostResult<Vec<u8>>
//! ```

pub mod abi;
pub mod audit;
pub mod error;
pub mod handler;
pub mod host;
pub mod manifest;
pub mod profile;

pub use ail_package::manifest::PackageManifest;
pub use ail_package::trust::TrustLevel;

pub use abi::{HostCallId, HostError, HostResult};
pub use audit::{AuditEvent, AuditLog};
pub use error::{PreflightFailure, RuntimeError, RuntimeResult};
pub use handler::{Handler, InMemoryHandler};
pub use host::{RuntimeHost, RuntimeInstance};
pub use manifest::{CapabilityManifest, blake3_hex_of};
pub use profile::{CapabilityGrant, CapabilityId, ResourceLimits, RuntimeProfile};
