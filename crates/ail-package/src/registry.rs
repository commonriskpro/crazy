// ── ail-package::registry ─────────────────────────────────────────────────
//
// `PackageRegistry` — in-memory store of `PackageManifest` entries.
//
// This is a local, in-memory registry only.  No network access or remote
// registry protocol is implemented here.  See Phase 16+ for remote support.
//
// # Determinism contract
//
// The registry stores manifests in insertion order (`Vec`).  `lookup_by_name_version`
// performs a linear scan — acceptable because registries are expected to be
// small (tens of entries) in practice.

use crate::manifest::PackageManifest;

// ── PackageRegistry ───────────────────────────────────────────────────────

/// In-memory registry of resolved `PackageManifest` entries.
///
/// Backed by a `Vec`; insertion order is preserved.  Use
/// [`PackageRegistry::register`] to add manifests and
/// [`PackageRegistry::lookup_by_name_version`] to query them.
#[derive(Clone, Debug, Default)]
pub struct PackageRegistry {
    manifests: Vec<PackageManifest>,
}

impl PackageRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        PackageRegistry::default()
    }

    /// Add a manifest to the registry.
    ///
    /// Duplicate entries (same name + version) are allowed; the last entry
    /// wins in a linear scan because `lookup_by_name_version` returns the
    /// first match (insertion-order priority).  Callers are responsible for
    /// ensuring uniqueness when that constraint matters.
    pub fn register(&mut self, manifest: PackageManifest) {
        self.manifests.push(manifest);
    }

    /// Look up a manifest by exact `name` and `version` match.
    ///
    /// Returns the first match in insertion order, or `None` if not found.
    pub fn lookup_by_name_version(&self, name: &str, version: &str) -> Option<&PackageManifest> {
        self.manifests
            .iter()
            .find(|m| m.name == name && m.version == version)
    }

    /// Return the total number of registered manifests.
    pub fn len(&self) -> usize {
        self.manifests.len()
    }

    /// Return `true` if no manifests have been registered.
    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }

    /// Return all registered manifests in insertion order.
    pub fn all(&self) -> &[PackageManifest] {
        &self.manifests
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{PackageDef, PackageManifest};
    use crate::trust::TrustLevel;

    fn make_manifest(name: &str, version: &str) -> PackageManifest {
        PackageManifest::from_def(PackageDef {
            name: name.to_string(),
            version: version.to_string(),
            trust_level: TrustLevel::Verified,
            required_capabilities: vec![],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
        })
    }

    // ── lookup_returns_registered_manifest ────────────────────────────────
    #[test]
    fn lookup_returns_registered_manifest() {
        let mut reg = PackageRegistry::new();
        let m = make_manifest("payments.stripe", "2.3.1");
        reg.register(m.clone());

        let found = reg.lookup_by_name_version("payments.stripe", "2.3.1");
        assert!(found.is_some(), "registered manifest must be findable");
        assert_eq!(found.unwrap(), &m);
    }

    // ── lookup_returns_none_for_unknown_package ───────────────────────────
    #[test]
    fn lookup_returns_none_for_unknown_package() {
        let reg = PackageRegistry::new();
        assert!(reg.lookup_by_name_version("unknown", "1.0.0").is_none());
    }

    // ── TRIANGULATE: version_mismatch_returns_none ────────────────────────
    #[test]
    fn version_mismatch_returns_none() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg", "1.0.0"));
        assert!(reg.lookup_by_name_version("pkg", "2.0.0").is_none());
    }

    // ── len_and_is_empty ─────────────────────────────────────────────────
    #[test]
    fn len_and_is_empty() {
        let mut reg = PackageRegistry::new();
        assert!(reg.is_empty());
        reg.register(make_manifest("a", "1.0.0"));
        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
    }
}
