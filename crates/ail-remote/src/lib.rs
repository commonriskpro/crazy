// ── ail-remote ────────────────────────────────────────────────────────────
//
// Remote collaboration primitives: agent identity, content bundles, and
// Ed25519-signed envelopes for `ContextResponse` and `CanonicalChangeSet`.
//
// # Crate contract
//
// `ail-remote` is a leaf crate.  It depends on `ail-storage` (for `ObjectId`
// and `CborCodec`) and `ail-change` / `ail-context` (for domain types) but
// MUST NOT depend on `ail-coordinator`, `ail-verify`, `ail-runtime`, or
// `ail-compiler`.
//
// # Workspace lint
//
// The workspace-level `deny(unsafe_code)` applies to our code only.
// `ed25519-dalek` uses `unsafe` internally in its own crate but does not
// require us to write any `unsafe` blocks.
//
// # Module overview
//
// - [`error`]    — `RemoteError` enum.
// - [`identity`] — `AgentIdentity`, `AgentKeypair`, `SigningError`.
// - [`bundle`]   — `ObjectBundle`, `BundleError`.
// - [`signing`]  — `SignedContextSlice`, `RemoteChangeSet`.

pub mod bundle;
pub mod error;
pub mod identity;
pub mod signing;

pub use bundle::{BundleError, ObjectBundle};
pub use error::RemoteError;
pub use identity::{AgentIdentity, AgentKeypair, SigningError};
pub use signing::{RemoteChangeSet, SignedContextSlice};
