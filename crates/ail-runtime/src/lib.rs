//! # ail-runtime
//!
//! Wasmtime-backed capability host for the AI-native language runtime.
//!
//! # What this crate does
//! - Owns the only direct `wasmtime` dependency in the workspace.
//! - Enforces a deny-by-default capability policy via preflight checks.
//! - Validates WASM module hashes and capability manifests before instantiation.
//! - Enforces input/output boundary schemas at capability call sites (G29 R2).
//! - Integrates transaction rollback with handler execution flow (G29 R2).
//! - Verifies replay output hashes against recorded BLAKE3 digests (G29 R2).
//! - Appends exactly one redacted [`AuditEvent`] per preflight call.
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
//!         │ check grant
//!         │ validate payload against CapabilityInputSchema (if registered)
//!         │ HandlerDispatch → Handler::handle
//!         │ validate response against CapabilityOutputSchema (if registered)
//!         │ append CapabilityCallExecuted
//!         ▼
//! HostResult<Vec<u8>>
//!
//! RuntimeHost::execute_with_rollback(tx, closure)
//!         │ run closure
//!         │ on Ok  → tx.commit()
//!         │ on Err → tx.rollback()
//!         ▼
//! HostResult<T>
//! ```

pub mod abi;
pub mod audit;
pub mod codec;
pub mod error;
pub mod handler;
pub mod host;
pub mod manifest;
pub mod profile;
pub mod replay;
pub mod report;
pub mod schema;
pub mod transaction;

pub use ail_package::manifest::PackageManifest;
pub use ail_package::trust::TrustLevel;

pub use abi::{HostCallId, HostError, HostResult};
pub use codec::{HandleId, HandleRegistry, StructuredValue, ValueDecoder, ValueLayout};
pub use audit::{AuditEvent, AuditLog};
pub use error::{PreflightFailure, RuntimeError, RuntimeResult};
pub use handler::{ClockHandler, Handler, InMemoryHandler, LogHandler};
pub use host::{CapabilityCallMode, RuntimeArg, RuntimeHost, RuntimeInstance, RuntimeValue, TraceContext};
pub use manifest::{CapabilityManifest, blake3_hex_of};
pub use profile::{
    AuditConfig, CapabilityGrant, CapabilityId, CapabilityRevocationRegistry, CapabilityState,
    InFlightPolicy, ProfilePolicy, RateLimit, ReplayConfig, ResourceLimits, RevocationRecord,
    RuntimeProfile, SecretEntry,
};
pub use replay::{
    FakePayment, FixedClock, InMemoryDb, RecordedHttp, ReplayEngine, ReplayHandler,
    ReplayVerificationError, SeededRandom, TamperTestHandler,
};
pub use report::{
    CapabilityCallSummary, LimitSnapshot, RuntimeCheck, RuntimeCheckResult, RuntimeReport,
    RuntimeReportStatus,
};
pub use schema::{
    CapabilityDefinition, CapabilityErrorSchema, CapabilityInputSchema, CapabilityOutputSchema,
    CapabilitySchema, SchemaField, SchemaValidationError, SchemaVariant,
};
pub use transaction::{
    CompensationPolicy, CompensationRequired, TransactionEntry, TransactionGroup,
    TransactionPolicy, TransactionStatus,
};
