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
        let prefix = self.qualified_prefix();
        symbol == prefix || symbol.starts_with(&format!("{prefix}."))
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

impl NamespaceOwnershipCheck {
    /// Check that `claimant` is permitted to export `symbol` given the
    /// registered namespace ownerships.
    ///
    /// A symbol is permitted if:
    /// - No namespace in `namespaces` contains it, OR
    /// - The namespace that contains it is owned by `claimant`.
    ///
    /// # Errors
    ///
    /// Returns `Err(OwnershipError::UnauthorizedNamespace)` if a namespace
    /// owned by a different package contains the symbol.
    pub fn check(
        symbol: &str,
        claimant: &str,
        namespaces: &[PackageNamespace],
    ) -> Result<(), OwnershipError> {
        for ns in namespaces {
            if ns.contains_symbol(symbol) && ns.owner != claimant {
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

    // ── namespace_kind_display ────────────────────────────────────────────
    #[test]
    fn namespace_kind_display() {
        assert_eq!(NamespaceKind::Package.to_string(), "pkg");
        assert_eq!(NamespaceKind::Type.to_string(), "type");
        assert_eq!(NamespaceKind::Handler.to_string(), "handler");
        assert_eq!(NamespaceKind::Capability.to_string(), "cap");
    }
}
