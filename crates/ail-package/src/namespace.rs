// ── ail-package::namespace ────────────────────────────────────────────────
//
// Namespace ownership and import alias stable identity.
//
// # Design (docs/packages.md §Namespaces)
//
// Packages own namespaces.  Qualified names follow the pattern:
//   pkg.payments.stripe
//   type.payments.stripe.PaymentRequest
//   handler.payments.stripe.StripePayment
//   cap.payments.stripe.payment.charge
//
// Imports can alias a package:
//   import payments.stripe as stripe
//
// Rule: Aliases do not change stable identity.  The stable identity is
// always the fully qualified package name; the alias is local only.
//
// # Design (docs/packages.md §Imports)
//
// The `ImportDeclaration` already stores `source_package` (stable identity).
// This module adds:
//   - `PackageNamespace` — the ownership record for a package namespace
//   - `ImportAlias`     — binds an alias to a stable package identity
//   - `NamespaceOwnershipCheck` — validates that a namespace segment is
//     owned by the expected package

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

// ── NamespaceKind ─────────────────────────────────────────────────────────

/// Kind of a qualified namespace segment.
///
/// Matches the prefix conventions from `docs/packages.md`:
///   `pkg.*`, `type.*`, `handler.*`, `cap.*`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NamespaceKind {
    /// Package root namespace (`pkg.*`).
    Package,
    /// Type namespace (`type.*`).
    Type,
    /// Handler namespace (`handler.*`).
    Handler,
    /// Capability namespace (`cap.*`).
    Capability,
}

impl std::fmt::Display for NamespaceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NamespaceKind::Package => write!(f, "pkg"),
            NamespaceKind::Type => write!(f, "type"),
            NamespaceKind::Handler => write!(f, "handler"),
            NamespaceKind::Capability => write!(f, "cap"),
        }
    }
}

// ── NamespaceValidation ───────────────────────────────────────────────────

/// Stable issue kind emitted by namespace validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NamespaceIssueKind {
    /// A package identity is not a non-empty dotted list of lowercase segments.
    InvalidPackageName,
    /// A namespace prefix is not a valid package-style namespace.
    InvalidNamespace,
    /// A local import alias is not a single lowercase segment.
    InvalidAlias,
    /// One alias points at more than one stable package identity.
    AliasCollision,
    /// A namespace prefix is outside the owning package prefix.
    NamespaceOwnerMismatch,
    /// A symbol is claimed from a namespace owned by a different package.
    UnauthorizedNamespace,
}

impl std::fmt::Display for NamespaceIssueKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NamespaceIssueKind::InvalidPackageName => write!(f, "invalid_package_name"),
            NamespaceIssueKind::InvalidNamespace => write!(f, "invalid_namespace"),
            NamespaceIssueKind::InvalidAlias => write!(f, "invalid_alias"),
            NamespaceIssueKind::AliasCollision => write!(f, "alias_collision"),
            NamespaceIssueKind::NamespaceOwnerMismatch => write!(f, "namespace_owner_mismatch"),
            NamespaceIssueKind::UnauthorizedNamespace => write!(f, "unauthorized_namespace"),
        }
    }
}

/// One deterministic namespace validation issue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamespaceIssue {
    /// Stable machine-readable issue kind.
    pub kind: NamespaceIssueKind,
    /// The package, namespace, alias, or symbol that failed validation.
    pub subject: String,
    /// Stable explanatory detail for human/debug output.
    pub detail: String,
}

impl NamespaceIssue {
    fn new(
        kind: NamespaceIssueKind,
        subject: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            subject: subject.into(),
            detail: detail.into(),
        }
    }
}

/// Validates package namespace and alias records with stable issue kinds.
pub struct NamespaceValidation;

impl NamespaceValidation {
    /// Validate one package identity such as `payments.stripe`.
    pub fn validate_package_name(package: &str) -> Vec<NamespaceIssue> {
        if is_dotted_lowercase_path(package) {
            Vec::new()
        } else {
            vec![NamespaceIssue::new(
                NamespaceIssueKind::InvalidPackageName,
                package,
                "package names must be non-empty dotted lowercase segments",
            )]
        }
    }

    /// Validate one namespace ownership record.
    pub fn validate_namespace(namespace: &PackageNamespace) -> Vec<NamespaceIssue> {
        let mut issues = Vec::new();

        if !is_dotted_lowercase_path(&namespace.owner) {
            issues.push(NamespaceIssue::new(
                NamespaceIssueKind::InvalidPackageName,
                &namespace.owner,
                "namespace owner must be a valid package name",
            ));
        }

        if !is_dotted_lowercase_path(&namespace.namespace) {
            issues.push(NamespaceIssue::new(
                NamespaceIssueKind::InvalidNamespace,
                &namespace.namespace,
                "namespace must be a valid package-style prefix",
            ));
        }

        if is_dotted_lowercase_path(&namespace.owner)
            && is_dotted_lowercase_path(&namespace.namespace)
            && namespace.namespace != namespace.owner
            && !namespace
                .namespace
                .starts_with(&format!("{}.", namespace.owner))
        {
            issues.push(NamespaceIssue::new(
                NamespaceIssueKind::NamespaceOwnerMismatch,
                &namespace.namespace,
                format!(
                    "namespace must equal owner '{}' or be below that prefix",
                    namespace.owner
                ),
            ));
        }

        sort_issues(&mut issues);
        issues
    }

    /// Validate import aliases and report ambiguous alias collisions once.
    pub fn validate_import_aliases(aliases: &[ImportAlias]) -> Vec<NamespaceIssue> {
        let mut issues = Vec::new();
        let mut packages_by_alias: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();

        for alias in aliases {
            if !is_dotted_lowercase_path(&alias.package) {
                issues.push(NamespaceIssue::new(
                    NamespaceIssueKind::InvalidPackageName,
                    &alias.package,
                    "alias package must be a valid package name",
                ));
            }

            if !is_lowercase_segment(&alias.alias) || is_reserved_namespace_prefix(&alias.alias) {
                issues.push(NamespaceIssue::new(
                    NamespaceIssueKind::InvalidAlias,
                    &alias.alias,
                    "aliases must be single lowercase non-reserved segments",
                ));
            }

            packages_by_alias
                .entry(alias.alias.as_str())
                .or_default()
                .insert(alias.package.as_str());
        }

        for (alias, packages) in packages_by_alias {
            if packages.len() > 1 {
                issues.push(NamespaceIssue::new(
                    NamespaceIssueKind::AliasCollision,
                    alias,
                    format!(
                        "alias maps to multiple packages: {}",
                        packages.into_iter().collect::<Vec<_>>().join(",")
                    ),
                ));
            }
        }

        sort_issues(&mut issues);
        issues
    }
}

fn is_dotted_lowercase_path(value: &str) -> bool {
    !value.is_empty() && value.split('.').all(is_lowercase_segment)
}

fn is_lowercase_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_lowercase())
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn is_reserved_namespace_prefix(segment: &str) -> bool {
    matches!(segment, "pkg" | "type" | "handler" | "cap")
}

fn sort_issues(issues: &mut [NamespaceIssue]) {
    issues.sort_by(|a, b| {
        (a.kind, a.subject.as_str(), a.detail.as_str()).cmp(&(
            b.kind,
            b.subject.as_str(),
            b.detail.as_str(),
        ))
    });
}

// ── PackageNamespace ──────────────────────────────────────────────────────

/// Ownership record for a package namespace segment.
///
/// A package owns its qualified namespace.  No other package may publish
/// symbols under this namespace without creating a coherence violation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageNamespace {
    /// The owning package's stable identity (e.g., `"payments.stripe"`).
    pub owner: String,
    /// The qualified namespace prefix owned (e.g., `"payments.stripe"`).
    pub namespace: String,
    /// The namespace kind (package root, type, handler, capability).
    pub kind: NamespaceKind,
}

impl PackageNamespace {
    /// Build the fully qualified prefix for this namespace.
    ///
    /// For example, `kind=Type, namespace="payments.stripe"` produces
    /// `"type.payments.stripe"`.
    pub fn qualified_prefix(&self) -> String {
        format!("{}.{}", self.kind, self.namespace)
    }

    /// Return `true` if `symbol` lives within this namespace.
    ///
    /// A symbol is within the namespace if it starts with the qualified
    /// prefix followed by `.` or equals the prefix exactly.
    pub fn contains_symbol(&self, symbol: &str) -> bool {
        self.symbol_match_specificity(symbol).is_some()
    }

    fn symbol_match_specificity(&self, symbol: &str) -> Option<usize> {
        let prefix = self.qualified_prefix();
        if symbol == prefix || symbol.starts_with(&format!("{prefix}.")) {
            Some(prefix.len())
        } else {
            None
        }
    }
}

// ── ImportAlias ───────────────────────────────────────────────────────────

/// A local alias for an imported package.
///
/// The alias is purely a compile-time convenience; the stable identity
/// (`package`) is always preserved.  Two imports with the same package
/// but different aliases are imports of the same package.
///
/// # Example (from docs/packages.md)
/// ```text
/// import payments.stripe as stripe
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportAlias {
    /// Stable package identity (e.g., `"payments.stripe"`).
    pub package: String,
    /// Local alias used in this importing package (e.g., `"stripe"`).
    pub alias: String,
}

impl ImportAlias {
    /// Return the stable identity regardless of alias.
    ///
    /// This enforces the rule: aliases do not change stable identity.
    pub fn stable_identity(&self) -> &str {
        &self.package
    }
}

// ── NamespaceOwnershipCheck ───────────────────────────────────────────────

/// Validates that namespace ownership is respected.
pub struct NamespaceOwnershipCheck;

/// Error returned by [`NamespaceOwnershipCheck::check`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnershipError {
    /// A symbol claims to be in a namespace owned by a different package.
    UnauthorizedNamespace {
        /// The symbol being checked.
        symbol: String,
        /// The namespace that owns that symbol prefix.
        namespace_owner: String,
        /// The package that is attempting to use the symbol.
        claimant: String,
    },
}

impl std::fmt::Display for OwnershipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OwnershipError::UnauthorizedNamespace {
                symbol,
                namespace_owner,
                claimant,
            } => write!(
                f,
                "symbol '{symbol}' is in namespace owned by '{namespace_owner}', \
                 but '{claimant}' is attempting to export it"
            ),
        }
    }
}

impl std::error::Error for OwnershipError {}

impl OwnershipError {
    /// Return the stable namespace issue kind for this ownership error.
    pub fn kind(&self) -> NamespaceIssueKind {
        match self {
            OwnershipError::UnauthorizedNamespace { .. } => {
                NamespaceIssueKind::UnauthorizedNamespace
            }
        }
    }
}

impl NamespaceOwnershipCheck {
    /// Check that `claimant` is permitted to export `symbol` given the
    /// registered namespace ownerships.
    ///
    /// A symbol is permitted if:
    /// - No namespace in `namespaces` contains it, OR
    /// - The most specific namespace that contains it is owned by `claimant`.
    ///
    /// # Errors
    ///
    /// Returns `Err(OwnershipError::UnauthorizedNamespace)` if the most
    /// specific namespace is owned by a different package.
    pub fn check(
        symbol: &str,
        claimant: &str,
        namespaces: &[PackageNamespace],
    ) -> Result<(), OwnershipError> {
        let matching_namespace = namespaces
            .iter()
            .filter_map(|ns| {
                ns.symbol_match_specificity(symbol)
                    .map(|specificity| (specificity, ns))
            })
            .max_by(
                |(left_specificity, left_ns), (right_specificity, right_ns)| {
                    left_specificity
                        .cmp(right_specificity)
                        .then_with(|| left_ns.qualified_prefix().cmp(&right_ns.qualified_prefix()))
                        .then_with(|| left_ns.owner.cmp(&right_ns.owner))
                },
            )
            .map(|(_, ns)| ns);

        if let Some(ns) = matching_namespace {
            if ns.owner != claimant {
                return Err(OwnershipError::UnauthorizedNamespace {
                    symbol: symbol.to_string(),
                    namespace_owner: ns.owner.clone(),
                    claimant: claimant.to_string(),
                });
            }
        }

        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── namespace_cbor_round_trip ─────────────────────────────────────────
    // Spec scenario: "PackageNamespace round-trips through CBOR"
    #[test]
    fn namespace_cbor_round_trip() {
        let ns = PackageNamespace {
            owner: "payments.stripe".to_string(),
            namespace: "payments.stripe".to_string(),
            kind: NamespaceKind::Package,
        };
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&ns, &mut buf).expect("encode");
        let decoded: PackageNamespace = ciborium::de::from_reader(buf.as_slice()).expect("decode");
        assert_eq!(decoded, ns);
    }

    // ── qualified_prefix_is_correct ───────────────────────────────────────
    // Spec scenario: Namespace kinds produce correct prefixes
    #[test]
    fn qualified_prefix_is_correct() {
        let pkg_ns = PackageNamespace {
            owner: "payments.stripe".to_string(),
            namespace: "payments.stripe".to_string(),
            kind: NamespaceKind::Package,
        };
        assert_eq!(pkg_ns.qualified_prefix(), "pkg.payments.stripe");

        let type_ns = PackageNamespace {
            owner: "payments.stripe".to_string(),
            namespace: "payments.stripe".to_string(),
            kind: NamespaceKind::Type,
        };
        assert_eq!(type_ns.qualified_prefix(), "type.payments.stripe");

        let handler_ns = PackageNamespace {
            owner: "payments.stripe".to_string(),
            namespace: "payments.stripe".to_string(),
            kind: NamespaceKind::Handler,
        };
        assert_eq!(handler_ns.qualified_prefix(), "handler.payments.stripe");

        let cap_ns = PackageNamespace {
            owner: "payments.stripe".to_string(),
            namespace: "payments.stripe".to_string(),
            kind: NamespaceKind::Capability,
        };
        assert_eq!(cap_ns.qualified_prefix(), "cap.payments.stripe");
    }

    // ── contains_symbol_detects_namespace_membership ──────────────────────
    // Spec scenario: Symbol within owned namespace is detected
    #[test]
    fn contains_symbol_detects_namespace_membership() {
        let ns = PackageNamespace {
            owner: "payments.stripe".to_string(),
            namespace: "payments.stripe".to_string(),
            kind: NamespaceKind::Type,
        };
        assert!(ns.contains_symbol("type.payments.stripe.PaymentRequest"));
        assert!(ns.contains_symbol("type.payments.stripe"));
        assert!(!ns.contains_symbol("type.payments.other.Foo"));
        assert!(!ns.contains_symbol("handler.payments.stripe.Bar"));
    }

    // ── import_alias_stable_identity ──────────────────────────────────────
    // Spec scenario: "import payments.stripe as stripe — alias does not change identity"
    //   GIVEN an ImportAlias with package="payments.stripe", alias="stripe"
    //   WHEN stable_identity() is called
    //   THEN it returns "payments.stripe" (not "stripe")
    #[test]
    fn import_alias_stable_identity() {
        let alias = ImportAlias {
            package: "payments.stripe".to_string(),
            alias: "stripe".to_string(),
        };
        assert_eq!(alias.stable_identity(), "payments.stripe");
        assert_ne!(alias.stable_identity(), alias.alias);
    }

    // ── import_alias_cbor_round_trip ──────────────────────────────────────
    // Spec scenario: ImportAlias round-trips through CBOR
    #[test]
    fn import_alias_cbor_round_trip() {
        let alias = ImportAlias {
            package: "payments.stripe".to_string(),
            alias: "stripe".to_string(),
        };
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&alias, &mut buf).expect("encode");
        let decoded: ImportAlias = ciborium::de::from_reader(buf.as_slice()).expect("decode");
        assert_eq!(decoded, alias);
    }

    // ── two_aliases_same_package_are_same_identity ────────────────────────
    // TRIANGULATE: different aliases for the same package share stable identity
    #[test]
    fn two_aliases_same_package_are_same_identity() {
        let a1 = ImportAlias {
            package: "payments.stripe".to_string(),
            alias: "stripe".to_string(),
        };
        let a2 = ImportAlias {
            package: "payments.stripe".to_string(),
            alias: "sp".to_string(),
        };
        assert_eq!(a1.stable_identity(), a2.stable_identity());
    }

    // ── ownership_check_passes_for_owner ─────────────────────────────────
    // Spec scenario: Package can export symbols in its own namespace
    //   GIVEN namespace owned by "payments.stripe"
    //   WHEN check() called with claimant="payments.stripe"
    //   THEN Ok(())
    #[test]
    fn ownership_check_passes_for_owner() {
        let namespaces = vec![PackageNamespace {
            owner: "payments.stripe".to_string(),
            namespace: "payments.stripe".to_string(),
            kind: NamespaceKind::Type,
        }];
        let result = NamespaceOwnershipCheck::check(
            "type.payments.stripe.PaymentRequest",
            "payments.stripe",
            &namespaces,
        );
        assert_eq!(result, Ok(()));
    }

    // ── ownership_check_fails_for_non_owner ───────────────────────────────
    // Spec scenario: Another package cannot export symbols in a foreign namespace
    //   GIVEN namespace owned by "payments.stripe"
    //   WHEN check() called with claimant="other.pkg"
    //   THEN Err(UnauthorizedNamespace)
    #[test]
    fn ownership_check_fails_for_non_owner() {
        let namespaces = vec![PackageNamespace {
            owner: "payments.stripe".to_string(),
            namespace: "payments.stripe".to_string(),
            kind: NamespaceKind::Type,
        }];
        let result = NamespaceOwnershipCheck::check(
            "type.payments.stripe.PaymentRequest",
            "other.pkg",
            &namespaces,
        );
        assert!(
            matches!(result, Err(OwnershipError::UnauthorizedNamespace { .. })),
            "non-owner must not export in foreign namespace"
        );
    }

    // ── ownership_check_passes_when_no_matching_namespace ─────────────────
    // TRIANGULATE: symbol not in any registered namespace is always allowed
    #[test]
    fn ownership_check_passes_when_no_matching_namespace() {
        let namespaces = vec![PackageNamespace {
            owner: "payments.stripe".to_string(),
            namespace: "payments.stripe".to_string(),
            kind: NamespaceKind::Type,
        }];
        let result =
            NamespaceOwnershipCheck::check("type.utils.core.Result", "utils.core", &namespaces);
        assert_eq!(result, Ok(()));
    }

    // ── package_name_validation_rejects_invalid_segment_shapes ───────────
    // Production gate: package identities must be platform-stable lowercase paths.
    #[test]
    fn package_name_validation_rejects_invalid_segment_shapes() {
        for package in [
            "",
            "payments..stripe",
            "Payments.stripe",
            "payments.-stripe",
        ] {
            let issues = NamespaceValidation::validate_package_name(package);
            assert_eq!(issues.len(), 1, "{package} must produce one stable issue");
            assert_eq!(issues[0].kind, NamespaceIssueKind::InvalidPackageName);
        }
    }

    // ── namespace_validation_rejects_unowned_prefix ───────────────────────
    // Production gate: package namespace ownership cannot silently claim another prefix.
    #[test]
    fn namespace_validation_rejects_unowned_prefix() {
        let namespace = PackageNamespace {
            owner: "payments.stripe".to_string(),
            namespace: "accounts.paypal".to_string(),
            kind: NamespaceKind::Capability,
        };

        let issues = NamespaceValidation::validate_namespace(&namespace);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, NamespaceIssueKind::NamespaceOwnerMismatch);
        assert_eq!(issues[0].subject, "accounts.paypal");
    }

    // ── alias_validation_reports_collision_deterministically ──────────────
    // Production gate: one alias cannot ambiguously identify multiple packages.
    #[test]
    fn alias_validation_reports_collision_deterministically() {
        let aliases = vec![
            ImportAlias {
                package: "zeta.gateway".to_string(),
                alias: "stripe".to_string(),
            },
            ImportAlias {
                package: "payments.stripe".to_string(),
                alias: "stripe".to_string(),
            },
        ];
        let reversed = aliases.iter().cloned().rev().collect::<Vec<_>>();

        let issues = NamespaceValidation::validate_import_aliases(&aliases);
        assert_eq!(
            issues,
            NamespaceValidation::validate_import_aliases(&reversed),
            "alias issue order and detail must not depend on import order"
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, NamespaceIssueKind::AliasCollision);
        assert_eq!(issues[0].subject, "stripe");
        assert_eq!(
            issues[0].detail,
            "alias maps to multiple packages: payments.stripe,zeta.gateway"
        );
    }

    // ── alias_validation_rejects_reserved_prefixes ────────────────────────
    // Production gate: aliases must not shadow namespace kind prefixes.
    #[test]
    fn alias_validation_rejects_reserved_prefixes() {
        let issues = NamespaceValidation::validate_import_aliases(&[ImportAlias {
            package: "payments.stripe".to_string(),
            alias: "type".to_string(),
        }]);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, NamespaceIssueKind::InvalidAlias);
        assert_eq!(issues[0].subject, "type");
    }

    // ── ownership_check_uses_most_specific_namespace ──────────────────────
    // Production gate: a parent package prefix must not steal a child namespace.
    #[test]
    fn ownership_check_uses_most_specific_namespace() {
        let namespaces = vec![
            PackageNamespace {
                owner: "payments".to_string(),
                namespace: "payments".to_string(),
                kind: NamespaceKind::Type,
            },
            PackageNamespace {
                owner: "payments.stripe".to_string(),
                namespace: "payments.stripe".to_string(),
                kind: NamespaceKind::Type,
            },
        ];

        assert_eq!(
            NamespaceOwnershipCheck::check(
                "type.payments.stripe.PaymentRequest",
                "payments.stripe",
                &namespaces,
            ),
            Ok(())
        );

        let error = NamespaceOwnershipCheck::check(
            "type.payments.stripe.PaymentRequest",
            "payments",
            &namespaces,
        )
        .expect_err("parent namespace must not own child symbol");
        assert_eq!(error.kind(), NamespaceIssueKind::UnauthorizedNamespace);
        assert!(matches!(
            error,
            OwnershipError::UnauthorizedNamespace {
                namespace_owner,
                ..
            } if namespace_owner == "payments.stripe"
        ));
    }

    // ── namespace_kind_display ────────────────────────────────────────────
    #[test]
    fn namespace_kind_display() {
        assert_eq!(NamespaceKind::Package.to_string(), "pkg");
        assert_eq!(NamespaceKind::Type.to_string(), "type");
        assert_eq!(NamespaceKind::Handler.to_string(), "handler");
        assert_eq!(NamespaceKind::Capability.to_string(), "cap");
    }
}
