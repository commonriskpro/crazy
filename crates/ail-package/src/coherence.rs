// ── ail-package::coherence ────────────────────────────────────────────────
//
// Cross-package interface implementation coherence / orphan rule enforcement.
//
// # Design (docs/packages.md §Orphan/coherence across packages)
//
// Interface impl coherence applies across packages.
//
// Rule:
//   A package can implement Interface<T> only if it owns the interface or
//   owns T, unless an explicit adapter/newtype is created.
//
// Conflicting implementations are compile errors.
//
// # Implementation model
//
// An `InterfaceImpl` records:
//   - the implementing package
//   - the interface being implemented
//   - the concrete type T
//   - whether it was declared as an adapter/newtype (exception to the rule)
//
// `CoherenceChecker` validates a set of `InterfaceImpl` records against a
// set of `PackageNamespace` ownerships, detecting:
//   1. Orphan violations: package implements Interface<T> without owning
//      either the interface or T.
//   2. Conflicting impls: two packages both provide the same Interface<T>
//      without an adapter.

use serde::{Deserialize, Serialize};

// ── InterfaceImpl ─────────────────────────────────────────────────────────

/// A declared implementation of `Interface<T>` by a package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceImpl {
    /// The package providing this implementation.
    pub implementor: String,
    /// Fully-qualified interface name (e.g., `"cap.payments.stripe.Chargeable"`).
    pub interface: String,
    /// Fully-qualified concrete type (e.g., `"type.payments.stripe.PaymentRequest"`).
    pub for_type: String,
    /// Whether this implementation uses a newtype/adapter pattern.
    ///
    /// If `true`, the orphan rule exception applies and the check passes
    /// even if `implementor` doesn't own either `interface` or `for_type`.
    pub is_adapter: bool,
}

// ── CoherenceError ────────────────────────────────────────────────────────

/// Error produced by [`CoherenceChecker::check`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoherenceError {
    /// A package implements an interface without owning the interface or the
    /// type, and without declaring an adapter.
    OrphanViolation {
        /// The package that violates the orphan rule.
        implementor: String,
        /// The interface being implemented.
        interface: String,
        /// The type argument.
        for_type: String,
    },
    /// Two packages both provide a non-adapter implementation of the same
    /// `Interface<T>`, creating a conflict.
    ConflictingImpl {
        /// First implementor.
        implementor_a: String,
        /// Second implementor.
        implementor_b: String,
        /// The conflicting interface.
        interface: String,
        /// The conflicting type.
        for_type: String,
    },
}

impl std::fmt::Display for CoherenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoherenceError::OrphanViolation {
                implementor,
                interface,
                for_type,
            } => write!(
                f,
                "orphan rule violation: '{implementor}' implements '{interface}' for '{for_type}' \
                 without owning either and without an adapter"
            ),
            CoherenceError::ConflictingImpl {
                implementor_a,
                implementor_b,
                interface,
                for_type,
            } => write!(
                f,
                "conflicting implementations: both '{implementor_a}' and '{implementor_b}' \
                 implement '{interface}' for '{for_type}'"
            ),
        }
    }
}

impl std::error::Error for CoherenceError {}

// ── CoherenceChecker ──────────────────────────────────────────────────────

/// Validates cross-package interface implementation coherence.
pub struct CoherenceChecker;

impl CoherenceChecker {
    /// Check that a package owns either the interface or the type for a given
    /// implementation.
    ///
    /// Returns `true` if `implementor` is a prefix of `symbol`'s namespace.
    fn package_owns(implementor: &str, symbol: &str) -> bool {
        // Strip the kind prefix (e.g., "type.", "cap.", "handler.", "pkg.")
        // and check if the remainder starts with the implementor's namespace.
        let bare = symbol.splitn(2, '.').nth(1).unwrap_or(symbol);
        bare == implementor || bare.starts_with(&format!("{implementor}."))
    }

    /// Validate a set of `InterfaceImpl` records for orphan violations and
    /// conflicting implementations.
    ///
    /// Validation order:
    /// 1. For each impl, check orphan rule (unless `is_adapter == true`).
    /// 2. For each pair of non-adapter impls with the same `interface` and
    ///    `for_type`, report a conflict.
    ///
    /// Returns all errors found (not just the first).
    pub fn check(impls: &[InterfaceImpl]) -> Vec<CoherenceError> {
        let mut errors = Vec::new();

        // Pass 1: orphan rule
        for impl_ in impls {
            if impl_.is_adapter {
                continue; // adapter exception
            }
            let owns_interface = Self::package_owns(&impl_.implementor, &impl_.interface);
            let owns_type = Self::package_owns(&impl_.implementor, &impl_.for_type);
            if !owns_interface && !owns_type {
                errors.push(CoherenceError::OrphanViolation {
                    implementor: impl_.implementor.clone(),
                    interface: impl_.interface.clone(),
                    for_type: impl_.for_type.clone(),
                });
            }
        }

        // Pass 2: conflict detection (non-adapter impls only)
        let non_adapter: Vec<&InterfaceImpl> = impls.iter().filter(|i| !i.is_adapter).collect();
        for i in 0..non_adapter.len() {
            for j in (i + 1)..non_adapter.len() {
                let a = non_adapter[i];
                let b = non_adapter[j];
                if a.interface == b.interface && a.for_type == b.for_type {
                    errors.push(CoherenceError::ConflictingImpl {
                        implementor_a: a.implementor.clone(),
                        implementor_b: b.implementor.clone(),
                        interface: a.interface.clone(),
                        for_type: a.for_type.clone(),
                    });
                }
            }
        }

        errors
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── interface_impl_cbor_round_trip ────────────────────────────────────
    // Spec scenario: InterfaceImpl round-trips through CBOR
    #[test]
    fn interface_impl_cbor_round_trip() {
        let impl_ = InterfaceImpl {
            implementor: "payments.stripe".to_string(),
            interface: "cap.payments.stripe.Chargeable".to_string(),
            for_type: "type.payments.stripe.PaymentRequest".to_string(),
            is_adapter: false,
        };
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&impl_, &mut buf).expect("encode");
        let decoded: InterfaceImpl = ciborium::de::from_reader(buf.as_slice()).expect("decode");
        assert_eq!(decoded, impl_);
    }

    // ── coherence_passes_for_interface_owner ──────────────────────────────
    // Spec scenario: Package owns the interface — orphan rule satisfied
    //   GIVEN InterfaceImpl where implementor owns the interface
    //   WHEN check() is called
    //   THEN no errors
    #[test]
    fn coherence_passes_for_interface_owner() {
        let impls = vec![InterfaceImpl {
            implementor: "payments.stripe".to_string(),
            interface: "cap.payments.stripe.Chargeable".to_string(),
            for_type: "type.utils.core.Request".to_string(),
            is_adapter: false,
        }];
        assert!(CoherenceChecker::check(&impls).is_empty());
    }

    // ── coherence_passes_for_type_owner ───────────────────────────────────
    // Spec scenario: Package owns the type — orphan rule satisfied
    #[test]
    fn coherence_passes_for_type_owner() {
        let impls = vec![InterfaceImpl {
            implementor: "utils.core".to_string(),
            interface: "cap.payments.stripe.Chargeable".to_string(),
            for_type: "type.utils.core.Request".to_string(),
            is_adapter: false,
        }];
        assert!(CoherenceChecker::check(&impls).is_empty());
    }

    // ── coherence_fails_orphan_rule ───────────────────────────────────────
    // Spec scenario: Package owns neither interface nor type — orphan violation
    //   GIVEN InterfaceImpl where implementor is "other.pkg" but interface and type
    //         belong to different packages
    //   WHEN check() is called
    //   THEN returns OrphanViolation error
    #[test]
    fn coherence_fails_orphan_rule() {
        let impls = vec![InterfaceImpl {
            implementor: "other.pkg".to_string(),
            interface: "cap.payments.stripe.Chargeable".to_string(),
            for_type: "type.utils.core.Request".to_string(),
            is_adapter: false,
        }];
        let errors = CoherenceChecker::check(&impls);
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], CoherenceError::OrphanViolation { implementor, .. } if implementor == "other.pkg")
        );
    }

    // ── adapter_exempts_from_orphan_rule ──────────────────────────────────
    // Spec scenario: Adapter/newtype pattern exempts from orphan rule
    //   GIVEN InterfaceImpl with is_adapter=true and no ownership
    //   WHEN check() is called
    //   THEN no errors
    #[test]
    fn adapter_exempts_from_orphan_rule() {
        let impls = vec![InterfaceImpl {
            implementor: "other.pkg".to_string(),
            interface: "cap.payments.stripe.Chargeable".to_string(),
            for_type: "type.utils.core.Request".to_string(),
            is_adapter: true,
        }];
        assert!(CoherenceChecker::check(&impls).is_empty());
    }

    // ── conflicting_impls_detected ────────────────────────────────────────
    // Spec scenario: Two non-adapter implementations of the same Interface<T>
    //   GIVEN two non-adapter impls for the same interface and type by different packages
    //   WHEN check() is called
    //   THEN returns ConflictingImpl error
    #[test]
    fn conflicting_impls_detected() {
        let impls = vec![
            InterfaceImpl {
                implementor: "payments.stripe".to_string(),
                interface: "cap.payments.Chargeable".to_string(),
                for_type: "type.payments.stripe.Card".to_string(),
                is_adapter: false,
            },
            InterfaceImpl {
                implementor: "payments.stripe".to_string(), // owns the type
                interface: "cap.payments.Chargeable".to_string(),
                for_type: "type.payments.stripe.Card".to_string(),
                is_adapter: false,
            },
        ];
        let errors = CoherenceChecker::check(&impls);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, CoherenceError::ConflictingImpl { .. })),
            "duplicate non-adapter impl must be detected as conflicting"
        );
    }

    // ── adapter_does_not_conflict_with_non_adapter ────────────────────────
    // TRIANGULATE: adapter and non-adapter for same Interface<T> do NOT conflict
    #[test]
    fn adapter_does_not_conflict_with_non_adapter() {
        let impls = vec![
            InterfaceImpl {
                implementor: "payments.stripe".to_string(),
                interface: "cap.payments.stripe.Chargeable".to_string(),
                for_type: "type.payments.stripe.Card".to_string(),
                is_adapter: false,
            },
            InterfaceImpl {
                implementor: "other.pkg".to_string(),
                interface: "cap.payments.stripe.Chargeable".to_string(),
                for_type: "type.payments.stripe.Card".to_string(),
                is_adapter: true, // adapter — no conflict
            },
        ];
        let errors = CoherenceChecker::check(&impls);
        assert!(
            !errors
                .iter()
                .any(|e| matches!(e, CoherenceError::ConflictingImpl { .. })),
            "adapter must not count as conflicting impl"
        );
    }

    // ── no_impls_produces_no_errors ───────────────────────────────────────
    // TRIANGULATE: empty impl list produces no errors
    #[test]
    fn no_impls_produces_no_errors() {
        assert!(CoherenceChecker::check(&[]).is_empty());
    }
}
