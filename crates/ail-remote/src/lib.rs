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
// - [`policy`]   — remote signer allowlist policy DTOs.
// - [`signing`]  — `SignedContextSlice`, `RemoteChangeSet`.
// - [`crypto`]   — AES-256-GCM, Argon2id, X25519 primitives (feature = "crypto").

pub mod bundle;
#[cfg(feature = "crypto")]
pub mod crypto;
pub mod error;
pub mod exchange;
pub mod identity;
pub mod policy;
pub mod signing;

pub use bundle::{BundleError, ObjectBundle};
#[cfg(feature = "crypto")]
pub use crypto::{
    CryptoError, decrypt_aes256gcm, derive_key_argon2, encrypt_aes256gcm, x25519_shared_secret,
};
pub use error::RemoteError;
pub use exchange::{RemoteExchangeRequest, RemoteExchangeResponse, RemoteSubmissionOutcome};
pub use identity::{AgentIdentity, AgentKeypair, SigningError};
pub use policy::{
    RemoteSignerPolicy, RemoteSignerRejection, RemoteSignerRejectionReason, SignerTrustTier,
    TrustedRemoteSigner,
};
pub use signing::{RemoteChangeSet, SignedContextSlice};
