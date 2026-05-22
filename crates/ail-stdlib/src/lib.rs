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

/// Checked, wrapping, and saturating arithmetic (`std.numeric`).
pub mod numeric;

/// `Option<T>` combinators (`std.option`).
pub mod option;

/// `Result<T, E>` combinators (`std.result`).
pub mod result;

/// Text helpers (`std.text`).
pub mod text;

/// Effect-polymorphic iterator combinators (`std.iter`).
pub mod iter;

/// Canonical v1 stdlib module registry — `v1_registry()`.
pub mod v1;

pub use registry::{StabilityTier, StdlibEntry, StdlibError, StdlibId, StdlibRegistry};
pub use v1::{v1_registry, v1_registry_with_functions};
