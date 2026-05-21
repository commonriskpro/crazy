// ── ail-stdlib ────────────────────────────────────────────────────────────
//
// Canonical data crate for the AIL v1 standard-library registry.
//
// # Public API
//
// | Module | Contents |
// |--------|----------|
// | `registry` | `StabilityTier`, `StdlibId`, `StdlibEntry`, `StdlibRegistry`, `StdlibError` |
// | `capability` | `pub const &str` capability name constants |
// | `v1` | `v1_registry()` — the canonical 9-entry v1 stdlib registry |
//
// # Dependency isolation
//
// `ail-stdlib` depends only on `ail-core`, `serde`, `ciborium`, and `blake3`.
// It MUST NOT depend on `ail-verify`, `ail-compiler`, or `ail-runtime`.

/// Canonical capability name constants for `std.capability`.
pub mod capability;

/// Core registry types: `StabilityTier`, `StdlibId`, `StdlibEntry`,
/// `StdlibRegistry`, `StdlibError`, and all associated methods.
pub mod registry;

/// Canonical v1 stdlib module registry — `v1_registry()`.
pub mod v1;

pub use registry::{StabilityTier, StdlibEntry, StdlibError, StdlibId, StdlibRegistry};
pub use v1::v1_registry;
