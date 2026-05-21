// ── ail-package::surface ──────────────────────────────────────────────────
//
// `UnsafeSurfaceEntry` — a declared unsafe surface item for packages with
// `TrustLevel::Unsafe`.
//
// Packages at the `Unsafe` trust level MUST declare every unsafe surface
// item in their manifest.  An `Unsafe` package with an empty `unsafe_surface`
// list fails `PackageManifest::validate()`.

use serde::{Deserialize, Serialize};

// ── UnsafeSurfaceEntry ────────────────────────────────────────────────────

/// One item in the unsafe surface declaration of a package.
///
/// Each entry identifies a specific unsafe API, FFI call, or capability
/// usage that reviewers must examine before approving the package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnsafeSurfaceEntry {
    /// Category of the unsafe surface (e.g., `"ffi"`, `"raw-pointer"`, `"unsafe-block"`).
    pub kind: String,
    /// Qualified name of the unsafe symbol or site (e.g., `"libc::malloc"`).
    pub name: String,
    /// Human-readable rationale for why this surface exists.
    pub description: String,
}
