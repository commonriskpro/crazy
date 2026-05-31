// ── ail-package::resolver ─────────────────────────────────────────────────
//
// Dependency resolution with trust, advisory, yank, semver, schema, profile,
// capability/handler conflict, and license policy checks.
//
// # Design (docs/packages.md §Dependency resolution)
//
// Resolver considers:
//   version constraints (semver ranges)
//   schema compatibility
//   trust requirements
//   profile policy
//   capability conflicts
//   handler conflicts
//   license policy
//
// - `DependencyResolver` is a stateless unit struct.  All inputs are passed
//   per-call.
// - Resolution order: NotFound → Yanked → Advisory → TrustViolation →
//   ProfilePolicy → LicensePolicy → CapabilityConflict → HandlerConflict → Ok.
// - Version matching uses semver VersionReq for range constraints.

use std::collections::{BTreeMap, BTreeSet};

use semver::{Version, VersionReq};

use crate::advisory::SecurityAdvisory;
use crate::manifest::PackageManifest;
use crate::policy::DeploymentProfile;
use crate::policy::TrustGate;
use crate::policy::TrustGateVerdict;
use crate::registry::PackageRegistry;
use crate::trust::TrustLevel;
use crate::yank::YankRecord;

// ── DependencySpec ────────────────────────────────────────────────────────

/// A declared dependency with version constraint and trust/policy requirements.
///
/// `version_constraint` is a semver VersionReq string (e.g., `"^1.2"`,
/// `">=1.0.0"`) or an exact version string (e.g., `"1.2.0"`).
/// `min_trust` is the minimum `TrustLevel` required for resolution to succeed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencySpec {
    /// Package name (e.g., `"payments.stripe"`).
    pub name: String,
    /// SemVer constraint string (e.g., `"^1.2"`, `"1.2.0"`, `">=1.0.0 <2.0.0"`).
    pub version_constraint: String,
    /// Minimum trust level required (e.g., `TrustLevel::Assumed`).
    pub min_trust: TrustLevel,
    /// Deployment profile to apply trust gate checks.
    pub profile: Option<DeploymentProfile>,
    /// SPDX license expressions that are allowed (empty = any license allowed).
    ///
    /// If non-empty, the resolved package's license must appear in this list.
    pub allowed_licenses: Vec<String>,
    /// Capability IDs that are disallowed in resolved packages.
    ///
    /// If a resolved package requests any of these capabilities, resolution fails.
    pub denied_capabilities: Vec<String>,
    /// Handler names that conflict and cannot coexist.
    ///
    /// If a resolved package exports any of these handlers, resolution fails.
    pub denied_handlers: Vec<String>,
    /// Minimum graph schema version required for schema compatibility.
    pub min_graph_schema: Option<u32>,
    /// Minimum core IR schema version required for schema compatibility.
    pub min_core_ir_schema: Option<u32>,
}

// ── ResolverError ─────────────────────────────────────────────────────────

/// Errors returned by [`DependencyResolver::resolve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolverError {
    /// No package with the requested name and version was found in the registry.
    NotFound {
        /// Package name that was not found.
        name: String,
        /// Constraint that produced no match.
        version_constraint: String,
    },
    /// The package was yanked and cannot be used for new resolution.
    Yanked {
        /// Human-readable reason for the yank.
        reason: String,
    },
    /// The package matches a security advisory.
    Advisory {
        /// Advisory identifier.
        id: String,
        /// Advisory severity.
        severity: crate::advisory::AdvisorySeverity,
    },
    /// The package's trust level does not meet the minimum required.
    TrustViolation {
        /// The actual trust level of the resolved package.
        actual: TrustLevel,
        /// The minimum trust level required by the dependency spec.
        required: TrustLevel,
    },
    /// The package is blocked by the profile-based trust gate.
    ProfilePolicyViolation {
        /// The deployment profile that blocked the package.
        profile: DeploymentProfile,
        /// The package trust level.
        trust_level: TrustLevel,
    },
    /// The package's license is not in the allowed list.
    LicenseViolation {
        /// The package's actual license.
        actual_license: Option<String>,
    },
    /// The package requests a capability that is disallowed.
    CapabilityConflict {
        /// The disallowed capability.
        capability: String,
    },
    /// The package exports a handler that conflicts with policy.
    HandlerConflict {
        /// The conflicting handler name.
        handler: String,
    },
    /// The package's schema version is incompatible.
    SchemaIncompatible {
        /// Human-readable reason (e.g., "graph_schema 1 < required 3").
        reason: String,
    },
    /// Two dependency specs resolved the same package to incompatible versions.
    ConflictingVersion {
        /// Package name with conflicting resolved versions.
        name: String,
        /// First resolved version.
        v1: String,
        /// Second resolved version.
        v2: String,
    },
}

impl std::fmt::Display for ResolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolverError::NotFound {
                name,
                version_constraint,
            } => {
                write!(
                    f,
                    "package {name} matching '{version_constraint}' not found in registry"
                )
            }
            ResolverError::Yanked { reason } => {
                write!(f, "package is yanked: {reason}")
            }
            ResolverError::Advisory { id, severity } => {
                write!(
                    f,
                    "security advisory {id} ({severity}) affects this package"
                )
            }
            ResolverError::TrustViolation { actual, required } => {
                write!(
                    f,
                    "trust violation: package has trust {actual} but {required} is required"
                )
            }
            ResolverError::ProfilePolicyViolation {
                profile,
                trust_level,
            } => {
                write!(
                    f,
                    "profile policy violation: {trust_level} package is denied in {profile} profile"
                )
            }
            ResolverError::LicenseViolation { actual_license } => {
                let lic = actual_license.as_deref().unwrap_or("<none>");
                write!(
                    f,
                    "license violation: package license '{lic}' is not in the allowed list"
                )
            }
            ResolverError::CapabilityConflict { capability } => {
                write!(
                    f,
                    "capability conflict: package requests disallowed capability '{capability}'"
                )
            }
            ResolverError::HandlerConflict { handler } => {
                write!(
                    f,
                    "handler conflict: package exports disallowed handler '{handler}'"
                )
            }
            ResolverError::SchemaIncompatible { reason } => {
                write!(f, "schema incompatible: {reason}")
            }
            ResolverError::ConflictingVersion { name, v1, v2 } => {
                write!(
                    f,
                    "conflicting versions for '{name}': resolved to both '{v1}' and '{v2}'"
                )
            }
        }
    }
}

impl std::error::Error for ResolverError {}

// ── ResolverDiagnostic ────────────────────────────────────────────────────

/// Stable diagnostic classes for package resolver conflict preflight checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResolverDiagnosticKind {
    /// The same package is declared more than once in the dependency input.
    DuplicatePackage,
    /// Multiple declarations for a package cannot be satisfied by one version.
    IncompatibleVersionRange,
    /// No registry entry exists for the requested package name.
    MissingPackage,
    /// The package exists, but no registered version satisfies the constraint.
    MissingVersion,
    /// Manifest import metadata forms a package dependency cycle.
    DependencyCycle,
    /// The same package/version appears with multiple source descriptors.
    ConflictingSource,
}

/// Redacted resolver conflict diagnostic suitable for production preflight logs.
///
/// Diagnostics intentionally expose package names, versions, and version
/// constraints only. Source descriptors are compared internally but never
/// emitted raw, because registry URLs may contain credentials or tenancy data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolverDiagnostic {
    /// Machine-readable diagnostic kind.
    pub kind: ResolverDiagnosticKind,
    /// Primary package name for the diagnostic.
    pub package: String,
    /// Primary package version, when the issue is version-specific.
    pub version: Option<String>,
    /// Relevant version constraints in canonical lexical order.
    pub constraints: Vec<String>,
    /// Relevant registry versions in canonical semantic order where possible.
    pub versions: Vec<String>,
    /// Related package names, such as a normalized cycle path.
    pub related_packages: Vec<String>,
    /// Stable human-readable summary without raw source descriptors.
    pub message: String,
    /// Always true for diagnostics that deliberately omit sensitive metadata.
    pub redacted: bool,
}

impl ResolverDiagnostic {
    fn duplicate_package(package: String, constraints: Vec<String>) -> Self {
        Self {
            kind: ResolverDiagnosticKind::DuplicatePackage,
            message: format!(
                "package '{package}' is declared multiple times with constraints [{}]",
                constraints.join(", ")
            ),
            package,
            version: None,
            constraints,
            versions: vec![],
            related_packages: vec![],
            redacted: true,
        }
    }

    fn incompatible_version_range(
        package: String,
        constraints: Vec<String>,
        versions: Vec<String>,
    ) -> Self {
        Self {
            kind: ResolverDiagnosticKind::IncompatibleVersionRange,
            message: format!(
                "package '{package}' constraints [{}] cannot be satisfied by one registered version",
                constraints.join(", ")
            ),
            package,
            version: None,
            constraints,
            versions,
            related_packages: vec![],
            redacted: true,
        }
    }

    fn missing_package(package: String, constraint: String) -> Self {
        Self {
            kind: ResolverDiagnosticKind::MissingPackage,
            message: format!(
                "package '{package}' is not present in the registry for constraint '{constraint}'"
            ),
            package,
            version: None,
            constraints: vec![constraint],
            versions: vec![],
            related_packages: vec![],
            redacted: true,
        }
    }

    fn missing_version(package: String, constraint: String, versions: Vec<String>) -> Self {
        Self {
            kind: ResolverDiagnosticKind::MissingVersion,
            message: format!(
                "package '{package}' has no registered version matching constraint '{constraint}'"
            ),
            package,
            version: None,
            constraints: vec![constraint],
            versions,
            related_packages: vec![],
            redacted: true,
        }
    }

    fn dependency_cycle(package: String, related_packages: Vec<String>) -> Self {
        Self {
            kind: ResolverDiagnosticKind::DependencyCycle,
            message: format!(
                "package dependency cycle detected: {}",
                related_packages.join(" -> ")
            ),
            package,
            version: None,
            constraints: vec![],
            versions: vec![],
            related_packages,
            redacted: true,
        }
    }

    fn conflicting_source(package: String, version: String, source_count: usize) -> Self {
        Self {
            kind: ResolverDiagnosticKind::ConflictingSource,
            message: format!(
                "package '{package}' version '{version}' has {source_count} distinct source descriptors (redacted)"
            ),
            package,
            version: Some(version.clone()),
            constraints: vec![],
            versions: vec![version],
            related_packages: vec![],
            redacted: true,
        }
    }
}

// ── DependencyResolver ────────────────────────────────────────────────────

/// Stateless dependency resolver with full policy enforcement.
pub struct DependencyResolver;

impl DependencyResolver {
    /// Resolve a `VersionReq` string against all manifests in the registry
    /// for the named package, returning the best (highest) matching version.
    fn find_best_match<'a>(
        name: &str,
        version_constraint: &str,
        registry: &'a PackageRegistry,
    ) -> Option<&'a PackageManifest> {
        // Parse as VersionReq; fall back to exact match.
        let req = VersionReq::parse(version_constraint).ok();

        let mut best: Option<&PackageManifest> = None;
        let mut best_ver: Option<Version> = None;

        for m in registry.all() {
            if m.name != name {
                continue;
            }
            let Ok(ver) = Version::parse(&m.version) else {
                // Not a valid semver — treat as exact match fallback.
                if version_constraint == m.version {
                    return Some(m);
                }
                continue;
            };
            let matches = match &req {
                Some(r) => r.matches(&ver),
                None => version_constraint == m.version,
            };
            if matches {
                match &best_ver {
                    None => {
                        best = Some(m);
                        best_ver = Some(ver);
                    }
                    Some(prev) if ver > *prev => {
                        best = Some(m);
                        best_ver = Some(ver);
                    }
                    _ => {}
                }
            }
        }
        best
    }

    /// Return conflict versions in canonical order so diagnostics do not depend
    /// on the input dependency declaration order.
    fn canonical_conflict_versions<'a>(left: &'a str, right: &'a str) -> (&'a str, &'a str) {
        match (Version::parse(left), Version::parse(right)) {
            (Ok(left_ver), Ok(right_ver)) if right_ver < left_ver => (right, left),
            (Ok(_), Ok(_)) => (left, right),
            _ if right < left => (right, left),
            _ => (left, right),
        }
    }

    fn version_matches_constraint(version: &str, constraint: &str) -> bool {
        let Ok(parsed_version) = Version::parse(version) else {
            return version == constraint;
        };

        VersionReq::parse(constraint)
            .map(|req| req.matches(&parsed_version))
            .unwrap_or_else(|_| version == constraint)
    }

    fn registry_versions_for(package: &str, registry: &PackageRegistry) -> Vec<String> {
        let mut versions = BTreeSet::new();
        for manifest in registry.all() {
            if manifest.name == package {
                versions.insert(manifest.version.clone());
            }
        }
        Self::canonical_versions(versions)
    }

    fn canonical_versions(versions: BTreeSet<String>) -> Vec<String> {
        let mut versions: Vec<String> = versions.into_iter().collect();
        versions.sort_by(
            |left, right| match (Version::parse(left), Version::parse(right)) {
                (Ok(left_version), Ok(right_version)) => left_version.cmp(&right_version),
                _ => left.cmp(right),
            },
        );
        versions
    }

    fn diagnostic_sort_key(
        diagnostic: &ResolverDiagnostic,
    ) -> (
        String,
        ResolverDiagnosticKind,
        Option<String>,
        Vec<String>,
        Vec<String>,
        Vec<String>,
    ) {
        (
            diagnostic.package.clone(),
            diagnostic.kind,
            diagnostic.version.clone(),
            diagnostic.constraints.clone(),
            diagnostic.versions.clone(),
            diagnostic.related_packages.clone(),
        )
    }

    fn provenance_source_descriptor(manifest: &PackageManifest) -> Option<String> {
        let provenance = manifest.provenance.as_ref()?;
        if provenance.source_repository.is_none() && provenance.url.is_none() {
            return None;
        }

        Some(format!(
            "source_repository={:?};url={:?}",
            provenance.source_repository, provenance.url
        ))
    }

    fn add_duplicate_package_diagnostics(
        specs_by_package: &BTreeMap<String, Vec<&DependencySpec>>,
        diagnostics: &mut Vec<ResolverDiagnostic>,
    ) {
        for (package, specs) in specs_by_package {
            if specs.len() <= 1 {
                continue;
            }

            let constraints = specs
                .iter()
                .map(|spec| spec.version_constraint.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            diagnostics.push(ResolverDiagnostic::duplicate_package(
                package.clone(),
                constraints,
            ));
        }
    }

    fn add_missing_diagnostics(
        specs: &[DependencySpec],
        registry: &PackageRegistry,
        diagnostics: &mut Vec<ResolverDiagnostic>,
    ) {
        let mut emitted = BTreeSet::new();

        for spec in specs {
            let versions = Self::registry_versions_for(&spec.name, registry);
            let key = (spec.name.clone(), spec.version_constraint.clone());
            if !emitted.insert(key) {
                continue;
            }

            if versions.is_empty() {
                diagnostics.push(ResolverDiagnostic::missing_package(
                    spec.name.clone(),
                    spec.version_constraint.clone(),
                ));
                continue;
            }

            if !versions
                .iter()
                .any(|version| Self::version_matches_constraint(version, &spec.version_constraint))
            {
                diagnostics.push(ResolverDiagnostic::missing_version(
                    spec.name.clone(),
                    spec.version_constraint.clone(),
                    versions,
                ));
            }
        }
    }

    fn add_incompatible_range_diagnostics(
        specs_by_package: &BTreeMap<String, Vec<&DependencySpec>>,
        registry: &PackageRegistry,
        diagnostics: &mut Vec<ResolverDiagnostic>,
    ) {
        for (package, specs) in specs_by_package {
            if specs.len() <= 1 {
                continue;
            }

            let versions = Self::registry_versions_for(package, registry);
            if versions.is_empty() {
                continue;
            }

            let constraints = specs
                .iter()
                .map(|spec| spec.version_constraint.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();

            let has_common_version = versions.iter().any(|version| {
                constraints
                    .iter()
                    .all(|constraint| Self::version_matches_constraint(version, constraint))
            });

            if !has_common_version {
                diagnostics.push(ResolverDiagnostic::incompatible_version_range(
                    package.clone(),
                    constraints,
                    versions,
                ));
            }
        }
    }

    fn add_conflicting_source_diagnostics(
        specs: &[DependencySpec],
        registry: &PackageRegistry,
        diagnostics: &mut Vec<ResolverDiagnostic>,
    ) {
        let package_scope = Self::diagnostic_package_scope(specs, registry);
        let mut sources_by_package_version: BTreeMap<(String, String), BTreeSet<String>> =
            BTreeMap::new();

        for manifest in registry.all() {
            if !package_scope.contains(&manifest.name) {
                continue;
            }

            if let Some(source) = Self::provenance_source_descriptor(manifest) {
                sources_by_package_version
                    .entry((manifest.name.clone(), manifest.version.clone()))
                    .or_default()
                    .insert(source);
            }
        }

        for ((package, version), sources) in sources_by_package_version {
            if sources.len() > 1 {
                diagnostics.push(ResolverDiagnostic::conflicting_source(
                    package,
                    version,
                    sources.len(),
                ));
            }
        }
    }

    fn dependency_graph_for(
        root_packages: &BTreeSet<String>,
        registry: &PackageRegistry,
    ) -> BTreeMap<String, BTreeSet<String>> {
        let registry_packages = registry
            .all()
            .iter()
            .map(|manifest| manifest.name.clone())
            .collect::<BTreeSet<_>>();
        let mut graph = BTreeMap::new();
        let mut pending = root_packages.iter().cloned().collect::<Vec<_>>();

        while let Some(package) = pending.pop() {
            if graph.contains_key(&package) {
                continue;
            }

            let mut imports = BTreeSet::new();
            for manifest in registry.all() {
                if manifest.name != package {
                    continue;
                }

                for import in &manifest.imports {
                    if registry_packages.contains(&import.source_package) {
                        imports.insert(import.source_package.clone());
                    }
                }
            }

            for import in &imports {
                if !graph.contains_key(import) {
                    pending.push(import.clone());
                }
            }
            graph.insert(package, imports);
        }

        graph
    }

    fn normalized_cycle(cycle: &[String]) -> Vec<String> {
        let Some((start_index, _)) = cycle
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| left.cmp(right))
        else {
            return vec![];
        };

        let mut normalized = cycle[start_index..].to_vec();
        normalized.extend_from_slice(&cycle[..start_index]);
        if let Some(first) = normalized.first().cloned() {
            normalized.push(first);
        }
        normalized
    }

    fn diagnostic_package_scope(
        specs: &[DependencySpec],
        registry: &PackageRegistry,
    ) -> BTreeSet<String> {
        let root_packages = specs
            .iter()
            .filter(|spec| {
                registry
                    .all()
                    .iter()
                    .any(|manifest| manifest.name == spec.name)
            })
            .map(|spec| spec.name.clone())
            .collect::<BTreeSet<_>>();
        let graph = Self::dependency_graph_for(&root_packages, registry);

        graph.into_keys().collect()
    }

    fn add_cycle_diagnostics(
        specs: &[DependencySpec],
        registry: &PackageRegistry,
        diagnostics: &mut Vec<ResolverDiagnostic>,
    ) {
        let package_scope = Self::diagnostic_package_scope(specs, registry);
        let graph = Self::dependency_graph_for(&package_scope, registry);
        let mut emitted_cycles = BTreeSet::new();

        for root in graph.keys() {
            let mut path = Vec::new();
            Self::visit_cycles(root, &graph, &mut path, &mut emitted_cycles, diagnostics);
        }
    }

    fn visit_cycles(
        package: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        path: &mut Vec<String>,
        emitted_cycles: &mut BTreeSet<Vec<String>>,
        diagnostics: &mut Vec<ResolverDiagnostic>,
    ) {
        if let Some(start_index) = path.iter().position(|visited| visited == package) {
            let normalized = Self::normalized_cycle(&path[start_index..]);
            if emitted_cycles.insert(normalized.clone()) {
                diagnostics.push(ResolverDiagnostic::dependency_cycle(
                    normalized.first().cloned().unwrap_or_default(),
                    normalized,
                ));
            }
            return;
        }

        path.push(package.to_string());
        if let Some(imports) = graph.get(package) {
            for import in imports {
                Self::visit_cycles(import, graph, path, emitted_cycles, diagnostics);
            }
        }
        path.pop();
    }

    /// Return stable redacted diagnostics for resolver preflight conflicts.
    ///
    /// This helper is intentionally parallel to [`resolve_all`](Self::resolve_all):
    /// it does not change the existing fail-fast resolver API, and callers that
    /// need production-ready reporting can run this before deciding whether to
    /// call `resolve_all`.
    pub fn conflict_diagnostics(
        specs: &[DependencySpec],
        registry: &PackageRegistry,
    ) -> Vec<ResolverDiagnostic> {
        let mut specs_by_package: BTreeMap<String, Vec<&DependencySpec>> = BTreeMap::new();
        for spec in specs {
            specs_by_package
                .entry(spec.name.clone())
                .or_default()
                .push(spec);
        }

        let mut diagnostics = Vec::new();
        Self::add_duplicate_package_diagnostics(&specs_by_package, &mut diagnostics);
        Self::add_missing_diagnostics(specs, registry, &mut diagnostics);
        Self::add_incompatible_range_diagnostics(&specs_by_package, registry, &mut diagnostics);
        Self::add_cycle_diagnostics(specs, registry, &mut diagnostics);
        Self::add_conflicting_source_diagnostics(specs, registry, &mut diagnostics);

        diagnostics.sort_by_key(Self::diagnostic_sort_key);
        diagnostics
    }

    /// Resolve a `DependencySpec` against the given registry, advisories, and
    /// yank records, enforcing the full policy chain from `docs/packages.md`.
    ///
    /// Resolution order:
    /// 1. Semver lookup: find best matching manifest → `ResolverError::NotFound`
    /// 2. Yank check → `ResolverError::Yanked`
    /// 3. Advisory check → `ResolverError::Advisory`
    /// 4. Trust level check → `ResolverError::TrustViolation`
    /// 5. Profile trust gate → `ResolverError::ProfilePolicyViolation`
    /// 6. Schema compatibility → `ResolverError::SchemaIncompatible`
    /// 7. License policy → `ResolverError::LicenseViolation`
    /// 8. Capability conflicts → `ResolverError::CapabilityConflict`
    /// 9. Handler conflicts → `ResolverError::HandlerConflict`
    /// 10. Otherwise → `Ok(&PackageManifest)`
    ///
    /// # Errors
    ///
    /// Returns the first `ResolverError` encountered per the resolution order.
    pub fn resolve<'a>(
        spec: &DependencySpec,
        registry: &'a PackageRegistry,
        advisories: &[SecurityAdvisory],
        yanks: &[YankRecord],
    ) -> Result<&'a PackageManifest, ResolverError> {
        // Step 1: semver lookup — find best matching manifest
        let manifest = Self::find_best_match(&spec.name, &spec.version_constraint, registry)
            .ok_or_else(|| ResolverError::NotFound {
                name: spec.name.clone(),
                version_constraint: spec.version_constraint.clone(),
            })?;

        // Step 2: yank check
        if let Some(yank) = yanks
            .iter()
            .find(|y| y.name == spec.name && y.version == manifest.version)
        {
            return Err(ResolverError::Yanked {
                reason: yank.reason.clone(),
            });
        }

        // Step 3: advisory check
        if let Some(adv) =
            crate::advisory::AdvisoryChecker::first_match(&spec.name, &manifest.version, advisories)
        {
            return Err(ResolverError::Advisory {
                id: adv.id.clone(),
                severity: adv.severity,
            });
        }

        // Step 4: trust level check
        if !manifest.trust_level.satisfies(spec.min_trust) {
            return Err(ResolverError::TrustViolation {
                actual: manifest.trust_level,
                required: spec.min_trust,
            });
        }

        // Step 5: profile trust gate
        if let Some(profile) = spec.profile
            && TrustGate::evaluate(manifest.trust_level, profile) == TrustGateVerdict::Deny
        {
            return Err(ResolverError::ProfilePolicyViolation {
                profile,
                trust_level: manifest.trust_level,
            });
        }

        // Step 6: schema compatibility
        if let Some(min_graph) = spec.min_graph_schema {
            let actual = manifest.graph_schema.unwrap_or(0);
            if actual < min_graph {
                return Err(ResolverError::SchemaIncompatible {
                    reason: format!("graph_schema {} < required {}", actual, min_graph),
                });
            }
        }
        if let Some(min_ir) = spec.min_core_ir_schema {
            let actual = manifest.core_ir_schema.unwrap_or(0);
            if actual < min_ir {
                return Err(ResolverError::SchemaIncompatible {
                    reason: format!("core_ir_schema {} < required {}", actual, min_ir),
                });
            }
        }

        // Step 7: license policy
        if !spec.allowed_licenses.is_empty() {
            let ok = manifest
                .license
                .as_ref()
                .is_some_and(|l| spec.allowed_licenses.iter().any(|a| a == l));
            if !ok {
                return Err(ResolverError::LicenseViolation {
                    actual_license: manifest.license.clone(),
                });
            }
        }

        // Step 8: capability conflicts
        for cap in &spec.denied_capabilities {
            if manifest.required_capabilities.contains(cap) {
                return Err(ResolverError::CapabilityConflict {
                    capability: cap.clone(),
                });
            }
        }

        // Step 9: handler conflicts
        for handler_name in &spec.denied_handlers {
            if manifest
                .handlers
                .iter()
                .any(|h| &h.handler_name == handler_name)
            {
                return Err(ResolverError::HandlerConflict {
                    handler: handler_name.clone(),
                });
            }
        }

        Ok(manifest)
    }

    /// Resolve multiple `DependencySpec`s against the registry.
    ///
    /// Calls [`resolve`](Self::resolve) for each spec, deduplicates by
    /// `name + version`, and returns an error for version conflicts (same
    /// package name resolved to two different versions).
    ///
    /// # Errors
    ///
    /// - Returns the first `ResolverError` from an individual `resolve()` call.
    /// - Returns `ResolverError::ConflictingVersion` if two specs resolve the
    ///   same package to different versions.  Diamond deps at the SAME version
    ///   are allowed (deduplicated to one manifest).
    pub fn resolve_all<'a>(
        specs: &[DependencySpec],
        registry: &'a PackageRegistry,
        advisories: &[SecurityAdvisory],
        yanks: &[YankRecord],
    ) -> Result<Vec<&'a PackageManifest>, ResolverError> {
        // Resolve each spec, collecting (name, version, manifest) triples.
        // Use a BTreeMap to detect conflicts deterministically.
        let mut resolved: std::collections::BTreeMap<String, (&'a PackageManifest, String)> =
            std::collections::BTreeMap::new();

        for spec in specs {
            let manifest = Self::resolve(spec, registry, advisories, yanks)?;
            match resolved.get(&manifest.name) {
                Some((_, prev_version)) if *prev_version != manifest.version => {
                    let (v1, v2) =
                        Self::canonical_conflict_versions(prev_version, &manifest.version);
                    return Err(ResolverError::ConflictingVersion {
                        name: manifest.name.clone(),
                        v1: v1.to_string(),
                        v2: v2.to_string(),
                    });
                }
                Some(_) => {
                    // Same name + version already resolved — deduplicate silently.
                }
                None => {
                    resolved.insert(manifest.name.clone(), (manifest, manifest.version.clone()));
                }
            }
        }

        Ok(resolved.into_values().map(|(m, _)| m).collect())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisory::{AdvisorySeverity, SecurityAdvisory};
    use crate::handler::HandlerExport;
    use crate::import::ImportDeclaration;
    use crate::manifest::{PackageDef, PackageManifest, Provenance};
    use crate::policy::DeploymentProfile;
    use crate::registry::PackageRegistry;
    use crate::trust::TrustLevel;
    use crate::yank::YankRecord;

    fn make_manifest(name: &str, version: &str, trust: TrustLevel) -> PackageManifest {
        PackageManifest::from_def(PackageDef {
            name: name.to_string(),
            version: version.to_string(),
            trust_level: trust,
            required_capabilities: vec![],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
            handlers: vec![],
            contracts: vec![],
            exports: vec![],
            imports: vec![],
            boundaries: vec![],
            license: None,
            provenance: None,
            verification_report: None,
            graph_schema: None,
            core_ir_schema: None,
            // 4G fields
            reproducible_evidence: None,
        })
    }

    fn spec(name: &str, version: &str, min_trust: TrustLevel) -> DependencySpec {
        DependencySpec {
            name: name.to_string(),
            version_constraint: version.to_string(),
            min_trust,
            profile: None,
            allowed_licenses: vec![],
            denied_capabilities: vec![],
            denied_handlers: vec![],
            min_graph_schema: None,
            min_core_ir_schema: None,
        }
    }

    fn import(source_package: &str) -> ImportDeclaration {
        ImportDeclaration {
            source_package: source_package.to_string(),
            items: vec![],
            version_constraint: None,
        }
    }

    fn manifest_with_imports(
        name: &str,
        version: &str,
        imports: Vec<ImportDeclaration>,
    ) -> PackageManifest {
        let mut manifest = make_manifest(name, version, TrustLevel::Verified);
        manifest.imports = imports;
        manifest
    }

    fn manifest_with_source(name: &str, version: &str, source: &str) -> PackageManifest {
        let mut manifest = make_manifest(name, version, TrustLevel::Verified);
        manifest.provenance = Some(Provenance::from_url(source));
        manifest
    }

    // ── RED: resolve_returns_manifest_for_valid_spec ──────────────────────
    // Spec: REQ-RES-3 — successful resolution returns the manifest
    //   GIVEN a registry with "payments.stripe" v1.2.0 (Verified)
    //   WHEN resolve is called with min_trust: Assumed, no advisories, no yanks
    //   THEN returns Ok pointing to the manifest
    #[test]
    fn resolve_returns_manifest_for_valid_spec() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest(
            "payments.stripe",
            "1.2.0",
            TrustLevel::Verified,
        ));

        let result = DependencyResolver::resolve(
            &spec("payments.stripe", "1.2.0", TrustLevel::Assumed),
            &reg,
            &[],
            &[],
        );

        assert!(result.is_ok(), "valid spec must resolve successfully");
        assert_eq!(result.unwrap().name, "payments.stripe");
    }

    // ── RED: resolve_returns_not_found_for_unknown_package ────────────────
    // Spec: REQ-RES-3 — NotFound when package is absent
    //   GIVEN an empty registry
    //   WHEN resolve is called for any package
    //   THEN returns ResolverError::NotFound
    #[test]
    fn resolve_returns_not_found_for_unknown_package() {
        let reg = PackageRegistry::new();
        let result = DependencyResolver::resolve(
            &spec("unknown.pkg", "1.0.0", TrustLevel::Unverified),
            &reg,
            &[],
            &[],
        );
        assert!(
            matches!(result, Err(ResolverError::NotFound { .. })),
            "absent package must return NotFound"
        );
    }

    // ── RED: resolve_returns_yanked_for_yanked_package ────────────────────
    // Spec: REQ-RES-4 / REQ-YANK-4 — Yanked packages are blocked in resolver
    //   GIVEN a registry with "pkg" v1.0.0 and a yank record for it
    //   WHEN resolve is called
    //   THEN returns ResolverError::Yanked
    #[test]
    fn resolve_returns_yanked_for_yanked_package() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg", "1.0.0", TrustLevel::Verified));
        let yanks = vec![YankRecord {
            name: "pkg".to_string(),
            version: "1.0.0".to_string(),
            reason: "security regression".to_string(),
        }];

        let result = DependencyResolver::resolve(
            &spec("pkg", "1.0.0", TrustLevel::Unverified),
            &reg,
            &[],
            &yanks,
        );
        assert!(
            matches!(result, Err(ResolverError::Yanked { .. })),
            "yanked package must return ResolverError::Yanked"
        );
    }

    // ── RED: resolve_returns_advisory_for_affected_package ────────────────
    // Spec: REQ-RES-4 — Advisory check blocks resolution
    //   GIVEN a registry with "stripe" v1.0.0 and a matching advisory
    //   WHEN resolve is called
    //   THEN returns ResolverError::Advisory
    #[test]
    fn resolve_returns_advisory_for_affected_package() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("stripe", "1.0.0", TrustLevel::Verified));
        let advisories = vec![SecurityAdvisory {
            id: "adv_007".to_string(),
            package: "stripe".to_string(),
            affected_constraint: "1.0.0".to_string(),
            severity: AdvisorySeverity::Critical,
            reason: "bug".to_string(),
        }];

        let result = DependencyResolver::resolve(
            &spec("stripe", "1.0.0", TrustLevel::Unverified),
            &reg,
            &advisories,
            &[],
        );
        assert!(
            matches!(result, Err(ResolverError::Advisory { id, .. }) if id == "adv_007"),
            "advisory match must return ResolverError::Advisory"
        );
    }

    // ── RED: resolve_returns_trust_violation_for_low_trust ────────────────
    // Spec: REQ-RES-4 — TrustViolation when package trust < required
    //   GIVEN a registry with "pkg" v2.0.0 at Unverified trust
    //   WHEN resolve is called requiring Assumed trust
    //   THEN returns ResolverError::TrustViolation
    #[test]
    fn resolve_returns_trust_violation_for_low_trust() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg", "2.0.0", TrustLevel::Unverified));

        let result =
            DependencyResolver::resolve(&spec("pkg", "2.0.0", TrustLevel::Assumed), &reg, &[], &[]);
        assert_eq!(
            result,
            Err(ResolverError::TrustViolation {
                actual: TrustLevel::Unverified,
                required: TrustLevel::Assumed,
            })
        );
    }

    // ── RED: yank_takes_precedence_over_advisory ──────────────────────────
    // TRIANGULATE: resolution order — Yanked before Advisory
    //   GIVEN a package that is both yanked AND has an advisory
    //   WHEN resolve is called
    //   THEN returns ResolverError::Yanked (not Advisory)
    #[test]
    fn yank_takes_precedence_over_advisory() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg", "1.0.0", TrustLevel::Verified));
        let yanks = vec![YankRecord {
            name: "pkg".to_string(),
            version: "1.0.0".to_string(),
            reason: "yanked".to_string(),
        }];
        let advisories = vec![SecurityAdvisory {
            id: "adv_x".to_string(),
            package: "pkg".to_string(),
            affected_constraint: "1.0.0".to_string(),
            severity: AdvisorySeverity::High,
            reason: "bug".to_string(),
        }];

        let result = DependencyResolver::resolve(
            &spec("pkg", "1.0.0", TrustLevel::Unverified),
            &reg,
            &advisories,
            &yanks,
        );
        assert!(
            matches!(result, Err(ResolverError::Yanked { .. })),
            "Yanked must take precedence over Advisory"
        );
    }

    // ── semver_range_resolves_best_matching_version ───────────────────────
    // Spec scenario: "DependencySpec with ^1.2 resolves best available 1.x"
    //   GIVEN registry with 1.2.0 and 1.5.0 (both Verified)
    //   WHEN resolve called with constraint "^1.2"
    //   THEN returns the highest matching version (1.5.0)
    #[test]
    fn semver_range_resolves_best_matching_version() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg", "1.2.0", TrustLevel::Verified));
        reg.register(make_manifest("pkg", "1.5.0", TrustLevel::Verified));
        reg.register(make_manifest("pkg", "2.0.0", TrustLevel::Verified));

        let s = DependencySpec {
            name: "pkg".to_string(),
            version_constraint: "^1.2".to_string(),
            min_trust: TrustLevel::Unverified,
            profile: None,
            allowed_licenses: vec![],
            denied_capabilities: vec![],
            denied_handlers: vec![],
            min_graph_schema: None,
            min_core_ir_schema: None,
        };
        let result = DependencyResolver::resolve(&s, &reg, &[], &[]);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().version,
            "1.5.0",
            "^1.2 must pick highest 1.x"
        );
    }

    // ── profile_policy_blocks_unverified_in_prod ──────────────────────────
    // Spec scenario: "Unverified package blocked in prod profile"
    #[test]
    fn profile_policy_blocks_unverified_in_prod() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg", "1.0.0", TrustLevel::Unverified));

        let s = DependencySpec {
            profile: Some(DeploymentProfile::Prod),
            ..spec("pkg", "1.0.0", TrustLevel::Unverified)
        };
        let result = DependencyResolver::resolve(&s, &reg, &[], &[]);
        assert!(
            matches!(result, Err(ResolverError::ProfilePolicyViolation { .. })),
            "unverified in prod must be ProfilePolicyViolation"
        );
    }

    // ── schema_incompatibility_blocked ────────────────────────────────────
    // Spec scenario: "Package with graph_schema 1 is blocked when >=3 required"
    #[test]
    fn schema_incompatibility_blocked() {
        let mut def = PackageDef {
            name: "pkg".to_string(),
            version: "1.0.0".to_string(),
            trust_level: TrustLevel::Verified,
            required_capabilities: vec![],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
            handlers: vec![],
            contracts: vec![],
            exports: vec![],
            imports: vec![],
            boundaries: vec![],
            license: None,
            provenance: None,
            verification_report: None,
            graph_schema: Some(1),
            core_ir_schema: None,
            // 4G fields
            reproducible_evidence: None,
        };
        let mut reg = PackageRegistry::new();
        reg.register(PackageManifest::from_def(def.clone()));

        let s = DependencySpec {
            min_graph_schema: Some(3),
            ..spec("pkg", "1.0.0", TrustLevel::Unverified)
        };
        let result = DependencyResolver::resolve(&s, &reg, &[], &[]);
        assert!(
            matches!(result, Err(ResolverError::SchemaIncompatible { .. })),
            "graph_schema 1 < 3 must fail"
        );

        // Now with sufficient graph_schema
        def.graph_schema = Some(3);
        let mut reg2 = PackageRegistry::new();
        reg2.register(PackageManifest::from_def(def));
        let s2 = DependencySpec {
            min_graph_schema: Some(3),
            ..spec("pkg", "1.0.0", TrustLevel::Unverified)
        };
        assert!(DependencyResolver::resolve(&s2, &reg2, &[], &[]).is_ok());
    }

    // ── license_policy_blocks_disallowed_license ──────────────────────────
    // Spec scenario: "Package with GPL license blocked when only MIT allowed"
    #[test]
    fn license_policy_blocks_disallowed_license() {
        let mut def = PackageDef {
            name: "pkg".to_string(),
            version: "1.0.0".to_string(),
            trust_level: TrustLevel::Verified,
            required_capabilities: vec![],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
            handlers: vec![],
            contracts: vec![],
            exports: vec![],
            imports: vec![],
            boundaries: vec![],
            license: Some("GPL-3.0".to_string()),
            provenance: None,
            verification_report: None,
            graph_schema: None,
            core_ir_schema: None,
            // 4G fields
            reproducible_evidence: None,
        };
        let mut reg = PackageRegistry::new();
        reg.register(PackageManifest::from_def(def.clone()));

        let s = DependencySpec {
            allowed_licenses: vec!["MIT".to_string(), "Apache-2.0".to_string()],
            ..spec("pkg", "1.0.0", TrustLevel::Unverified)
        };
        let result = DependencyResolver::resolve(&s, &reg, &[], &[]);
        assert!(
            matches!(result, Err(ResolverError::LicenseViolation { .. })),
            "GPL-3.0 not in [MIT, Apache-2.0] must fail"
        );

        // MIT is allowed
        def.license = Some("MIT".to_string());
        let mut reg2 = PackageRegistry::new();
        reg2.register(PackageManifest::from_def(def));
        let s2 = DependencySpec {
            allowed_licenses: vec!["MIT".to_string()],
            ..spec("pkg", "1.0.0", TrustLevel::Unverified)
        };
        assert!(DependencyResolver::resolve(&s2, &reg2, &[], &[]).is_ok());
    }

    // ── capability_conflict_blocked ───────────────────────────────────────
    // Spec scenario: "Package requesting denied capability is blocked"
    #[test]
    fn capability_conflict_blocked() {
        let def = PackageDef {
            name: "pkg".to_string(),
            version: "1.0.0".to_string(),
            trust_level: TrustLevel::Verified,
            required_capabilities: vec!["file.write:LocalDisk".to_string()],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
            handlers: vec![],
            contracts: vec![],
            exports: vec![],
            imports: vec![],
            boundaries: vec![],
            license: None,
            provenance: None,
            verification_report: None,
            graph_schema: None,
            core_ir_schema: None,
            // 4G fields
            reproducible_evidence: None,
        };
        let mut reg = PackageRegistry::new();
        reg.register(PackageManifest::from_def(def));

        let s = DependencySpec {
            denied_capabilities: vec!["file.write:LocalDisk".to_string()],
            ..spec("pkg", "1.0.0", TrustLevel::Unverified)
        };
        let result = DependencyResolver::resolve(&s, &reg, &[], &[]);
        assert!(
            matches!(result, Err(ResolverError::CapabilityConflict { capability }) if capability == "file.write:LocalDisk"),
            "denied capability must produce CapabilityConflict"
        );
    }

    // ── handler_conflict_blocked ──────────────────────────────────────────
    // Spec scenario: "Package exporting denied handler is blocked"
    #[test]
    fn handler_conflict_blocked() {
        let def = PackageDef {
            name: "pkg".to_string(),
            version: "1.0.0".to_string(),
            trust_level: TrustLevel::Verified,
            required_capabilities: vec![],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
            handlers: vec![HandlerExport {
                capability: "payment.charge:PaymentProvider".to_string(),
                handler_name: "StripePayment".to_string(),
                trust_level: TrustLevel::Verified,
            }],
            contracts: vec![],
            exports: vec![],
            imports: vec![],
            boundaries: vec![],
            license: None,
            provenance: None,
            verification_report: None,
            graph_schema: None,
            core_ir_schema: None,
            // 4G fields
            reproducible_evidence: None,
        };
        let mut reg = PackageRegistry::new();
        reg.register(PackageManifest::from_def(def));

        let s = DependencySpec {
            denied_handlers: vec!["StripePayment".to_string()],
            ..spec("pkg", "1.0.0", TrustLevel::Unverified)
        };
        let result = DependencyResolver::resolve(&s, &reg, &[], &[]);
        assert!(
            matches!(result, Err(ResolverError::HandlerConflict { handler }) if handler == "StripePayment"),
            "denied handler must produce HandlerConflict"
        );
    }

    // ── B7: resolve_all ────────────────────────────────────────────────────

    // Spec PKG-RES-1: two non-conflicting deps resolve to two manifests
    #[test]
    fn resolve_all_two_non_conflicting_deps() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg.a", "1.0.0", TrustLevel::Verified));
        reg.register(make_manifest("pkg.b", "2.0.0", TrustLevel::Verified));

        let specs = vec![
            spec("pkg.a", "1.0.0", TrustLevel::Unverified),
            spec("pkg.b", "2.0.0", TrustLevel::Unverified),
        ];
        let result = DependencyResolver::resolve_all(&specs, &reg, &[], &[]);
        assert!(result.is_ok(), "non-conflicting deps must resolve");
        let manifests = result.unwrap();
        assert_eq!(manifests.len(), 2);
    }

    // Spec PKG-RES-1: one failing dep propagates error
    #[test]
    fn resolve_all_one_failing_dep_propagates_error() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg.a", "1.0.0", TrustLevel::Verified));
        // pkg.b not in registry

        let specs = vec![
            spec("pkg.a", "1.0.0", TrustLevel::Unverified),
            spec("pkg.b", "1.0.0", TrustLevel::Unverified),
        ];
        let result = DependencyResolver::resolve_all(&specs, &reg, &[], &[]);
        assert!(
            matches!(result, Err(ResolverError::NotFound { .. })),
            "failing dep must propagate NotFound error"
        );
    }

    // Spec PKG-RES-1: same package at same version → deduplicated to one manifest
    #[test]
    fn resolve_all_deduplicates_same_name_same_version() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("utils.core", "1.0.0", TrustLevel::Verified));

        let specs = vec![
            spec("utils.core", "1.0.0", TrustLevel::Unverified),
            spec("utils.core", "1.0.0", TrustLevel::Unverified),
        ];
        let result = DependencyResolver::resolve_all(&specs, &reg, &[], &[]);
        assert!(result.is_ok(), "same name+version must deduplicate");
        let manifests = result.unwrap();
        assert_eq!(manifests.len(), 1, "deduplicated to one manifest");
    }

    // Spec PKG-RES-1: same package at conflicting versions → ConflictingVersion error
    #[test]
    fn resolve_all_conflicting_versions_produces_error() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("utils.core", "1.0.0", TrustLevel::Verified));
        reg.register(make_manifest("utils.core", "2.0.0", TrustLevel::Verified));

        let specs = vec![
            spec("utils.core", "1.0.0", TrustLevel::Unverified),
            spec("utils.core", "2.0.0", TrustLevel::Unverified),
        ];
        let result = DependencyResolver::resolve_all(&specs, &reg, &[], &[]);
        assert!(
            matches!(result, Err(ResolverError::ConflictingVersion { ref name, .. }) if name == "utils.core"),
            "conflicting versions must produce ConflictingVersion error"
        );
    }

    // Spec PKG-RES-REPRO-1: conflict diagnostics are independent of spec order.
    #[test]
    fn resolve_all_conflicting_versions_reports_canonical_versions() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("utils.core", "1.0.0", TrustLevel::Verified));
        reg.register(make_manifest("utils.core", "2.0.0", TrustLevel::Verified));

        let specs = vec![
            spec("utils.core", "2.0.0", TrustLevel::Unverified),
            spec("utils.core", "1.0.0", TrustLevel::Unverified),
        ];

        let result = DependencyResolver::resolve_all(&specs, &reg, &[], &[]);
        assert_eq!(
            result,
            Err(ResolverError::ConflictingVersion {
                name: "utils.core".to_string(),
                v1: "1.0.0".to_string(),
                v2: "2.0.0".to_string(),
            }),
            "conflict diagnostics must not depend on declaration order"
        );
    }

    // Spec PKG-RES-REPRO-2: resolved graph ordering is independent of spec order.
    #[test]
    fn resolve_all_returns_manifests_in_canonical_name_order() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg.z", "1.0.0", TrustLevel::Verified));
        reg.register(make_manifest("pkg.a", "1.0.0", TrustLevel::Verified));

        let specs = vec![
            spec("pkg.z", "1.0.0", TrustLevel::Unverified),
            spec("pkg.a", "1.0.0", TrustLevel::Unverified),
        ];

        let manifests = DependencyResolver::resolve_all(&specs, &reg, &[], &[])
            .expect("non-conflicting deps must resolve");

        assert_eq!(manifests.len(), 2);
        assert_eq!(manifests[0].name, "pkg.a");
        assert_eq!(manifests[1].name, "pkg.z");
    }

    // ── conflict_diagnostics ──────────────────────────────────────────────

    #[test]
    fn conflict_diagnostics_reports_duplicate_incompatible_and_missing_inputs() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg.alpha", "1.0.0", TrustLevel::Verified));
        reg.register(make_manifest("pkg.alpha", "2.0.0", TrustLevel::Verified));
        reg.register(make_manifest("pkg.beta", "1.0.0", TrustLevel::Verified));

        let specs = vec![
            spec("pkg.ghost", "1.0.0", TrustLevel::Unverified),
            spec("pkg.alpha", "2.0.0", TrustLevel::Unverified),
            spec("pkg.beta", "^2.0", TrustLevel::Unverified),
            spec("pkg.alpha", "1.0.0", TrustLevel::Unverified),
        ];

        let diagnostics = DependencyResolver::conflict_diagnostics(&specs, &reg);
        let summary = diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.package.as_str(), diagnostic.kind))
            .collect::<Vec<_>>();

        assert_eq!(
            summary,
            vec![
                ("pkg.alpha", ResolverDiagnosticKind::DuplicatePackage),
                (
                    "pkg.alpha",
                    ResolverDiagnosticKind::IncompatibleVersionRange
                ),
                ("pkg.beta", ResolverDiagnosticKind::MissingVersion),
                ("pkg.ghost", ResolverDiagnosticKind::MissingPackage),
            ],
            "diagnostics must be complete and canonical by package/kind"
        );
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.redacted));
        assert_eq!(
            diagnostics[1].constraints,
            vec!["1.0.0".to_string(), "2.0.0".to_string()]
        );
        assert_eq!(
            diagnostics[2].versions,
            vec!["1.0.0".to_string()],
            "missing-version diagnostics include safe registered version inventory"
        );
    }

    #[test]
    fn conflict_diagnostics_are_independent_of_input_order() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("pkg.alpha", "1.0.0", TrustLevel::Verified));
        reg.register(make_manifest("pkg.alpha", "2.0.0", TrustLevel::Verified));

        let specs_a = vec![
            spec("pkg.alpha", "2.0.0", TrustLevel::Unverified),
            spec("pkg.alpha", "1.0.0", TrustLevel::Unverified),
        ];
        let specs_b = vec![
            spec("pkg.alpha", "1.0.0", TrustLevel::Unverified),
            spec("pkg.alpha", "2.0.0", TrustLevel::Unverified),
        ];

        assert_eq!(
            DependencyResolver::conflict_diagnostics(&specs_a, &reg),
            DependencyResolver::conflict_diagnostics(&specs_b, &reg),
            "diagnostic output must not depend on dependency declaration order"
        );
    }

    #[test]
    fn conflict_diagnostics_report_dependency_cycles_when_imports_represent_them() {
        let mut reg = PackageRegistry::new();
        reg.register(manifest_with_imports(
            "pkg.a",
            "1.0.0",
            vec![import("pkg.b")],
        ));
        reg.register(manifest_with_imports(
            "pkg.b",
            "1.0.0",
            vec![import("pkg.a")],
        ));

        let diagnostics = DependencyResolver::conflict_diagnostics(
            &[spec("pkg.a", "1.0.0", TrustLevel::Unverified)],
            &reg,
        );

        let cycle = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == ResolverDiagnosticKind::DependencyCycle)
            .expect("cycle diagnostic");
        assert_eq!(cycle.package, "pkg.a");
        assert_eq!(
            cycle.related_packages,
            vec![
                "pkg.a".to_string(),
                "pkg.b".to_string(),
                "pkg.a".to_string()
            ]
        );
    }

    #[test]
    fn conflict_diagnostics_redact_conflicting_sources() {
        let mut reg = PackageRegistry::new();
        reg.register(manifest_with_source(
            "pkg.secret",
            "1.0.0",
            "https://token:abc@registry.example.test/pkg.secret",
        ));
        reg.register(manifest_with_source(
            "pkg.secret",
            "1.0.0",
            "https://token:def@mirror.example.test/pkg.secret",
        ));

        let diagnostics = DependencyResolver::conflict_diagnostics(
            &[spec("pkg.secret", "1.0.0", TrustLevel::Unverified)],
            &reg,
        );

        let source = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == ResolverDiagnosticKind::ConflictingSource)
            .expect("source conflict diagnostic");
        assert_eq!(source.package, "pkg.secret");
        assert_eq!(source.version.as_deref(), Some("1.0.0"));
        assert!(source.redacted);
        assert!(
            !format!("{source:?}").contains("token"),
            "diagnostic must not leak raw source credentials"
        );
    }

    // ── advisory_range_constraint_matches ─────────────────────────────────
    // Spec scenario: "Advisory with <1.2.3 blocks resolution of 1.0.0"
    #[test]
    fn advisory_range_constraint_blocks_resolution() {
        let mut reg = PackageRegistry::new();
        reg.register(make_manifest("stripe", "1.0.0", TrustLevel::Verified));
        let advisories = vec![SecurityAdvisory {
            id: "adv_range".to_string(),
            package: "stripe".to_string(),
            affected_constraint: "<1.2.3".to_string(),
            severity: AdvisorySeverity::Critical,
            reason: "bug".to_string(),
        }];

        let result = DependencyResolver::resolve(
            &spec("stripe", "1.0.0", TrustLevel::Unverified),
            &reg,
            &advisories,
            &[],
        );
        assert!(
            matches!(result, Err(ResolverError::Advisory { id, .. }) if id == "adv_range"),
            "<1.2.3 advisory must block 1.0.0"
        );
    }
}
