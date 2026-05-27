// ── Evidence codes ─────────────────────────────────────────────────────────

/// Nominal type mismatch at a call site.
pub const E_NOMINAL_MISMATCH: &str = "E_NOMINAL_MISMATCH";
/// Generic parameter name is empty (arity error).
pub const E_GENERIC_ARITY: &str = "E_GENERIC_ARITY";
/// EffectParam is declared but not reflected in the node's `effect_row`.
pub const E_EFFECT_PARAM_WIDENED: &str = "E_EFFECT_PARAM_WIDENED";
/// CapabilityParam is declared but not reflected in the node's `capability_reqs`.
pub const E_CAPABILITY_PARAM_WIDENED: &str = "E_CAPABILITY_PARAM_WIDENED";
/// Implicit coercion between parameterized types (variance violation).
pub const E_VARIANCE_COERCION: &str = "E_VARIANCE_COERCION";
/// Duplicate non-adapter implementation of the same interface on one node.
pub const E_COHERENCE_DUPLICATE: &str = "E_COHERENCE_DUPLICATE";
/// Associated type binding has an empty or invalid name.
pub const E_ASSOC_TYPE_MISMATCH: &str = "E_ASSOC_TYPE_MISMATCH";
/// Type used in a context requiring `Eq` but `has_eq` is false.
pub const E_MISSING_EQ: &str = "E_MISSING_EQ";
/// Type used in a context requiring `Hashable` but `has_hash` is false.
pub const E_MISSING_HASH: &str = "E_MISSING_HASH";
/// Type used in a context requiring `Ord` but `has_ord` is false.
pub const E_MISSING_ORD: &str = "E_MISSING_ORD";
/// Refinement erasure to base type occurred and is explicitly reported.
pub const E_REFINEMENT_ERASURE: &str = "E_REFINEMENT_ERASURE";
/// ConstParam name contains a complex/undecidable expression.
pub const E_CONST_PARAM_UNDECIDABLE: &str = "E_CONST_PARAM_UNDECIDABLE";
/// Function has declared params but no return_type — boundary not yet materialized.
pub const E_BOUNDARY_NOT_MATERIALIZED: &str = "E_BOUNDARY_NOT_MATERIALIZED";
/// Return type contains null/nil/undefined/void — prohibited in Core IR.
pub const E_NULL_IN_CORE_IR: &str = "E_NULL_IN_CORE_IR";
/// Float type has implicit equality (has_eq=true) without explicit comparator policy.
pub const E_FLOAT_EQ_IMPLICIT: &str = "E_FLOAT_EQ_IMPLICIT";
/// Float type has implicit ordering (has_ord=true) — Float has no default Ord.
pub const E_FLOAT_ORD_IMPLICIT: &str = "E_FLOAT_ORD_IMPLICIT";
/// Associated type binding has empty concrete type (ty field is empty).
pub const E_ASSOC_TYPE_EMPTY_BINDING: &str = "E_ASSOC_TYPE_EMPTY_BINDING";
/// Generic call-site bindings do not match the callee declaration arity.
pub const E_GENERIC_BINDING_ARITY: &str = "E_GENERIC_BINDING_ARITY";
/// Callee effects are not propagated to the caller effect row.
pub const E_EFFECT_NOT_PROPAGATED: &str = "E_EFFECT_NOT_PROPAGATED";
/// Callee capabilities are not propagated to the caller capability requirements.
pub const E_CAPABILITY_NOT_PROPAGATED: &str = "E_CAPABILITY_NOT_PROPAGATED";
/// Structural type requirement is not satisfied by the concrete argument type.
pub const E_STRUCTURAL_TYPE_MISMATCH: &str = "E_STRUCTURAL_TYPE_MISMATCH";
/// Dynamic interface dispatch target does not implement the requested interface.
pub const E_DYN_INTERFACE_UNAVAILABLE: &str = "E_DYN_INTERFACE_UNAVAILABLE";
/// Refinement status claims proof but predicate cannot be discharged locally.
pub const E_REFINEMENT_PROOF_UNDISCHARGED: &str = "E_REFINEMENT_PROOF_UNDISCHARGED";
/// Runtime-checked refinement has no materialized runtime check metadata.
pub const E_REFINEMENT_RUNTIME_CHECK_MISSING: &str = "E_REFINEMENT_RUNTIME_CHECK_MISSING";
/// PatchField type carries an empty inner type — prohibited in Core IR.
pub const E_PATCHFIELD_EMPTY_INNER: &str = "E_PATCHFIELD_EMPTY_INNER";
/// Boundary inferred fact return type does not match declared return_type.
pub const E_BOUNDARY_INFERENCE_MISMATCH: &str = "E_BOUNDARY_INFERENCE_MISMATCH";
/// Type used in a context requiring PartialOrd but only partial order is available.
pub const E_PARTIAL_ORD_REQUIRED: &str = "E_PARTIAL_ORD_REQUIRED";
/// Associated type reference in a callee return type could not be resolved
/// to a concrete type via any impl binding in the call context.
pub const E_ASSOC_TYPE_NOT_RESOLVED: &str = "E_ASSOC_TYPE_NOT_RESOLVED";
/// Function node has a ForeignType return type but no `boundary-schema` inferred
/// fact — the foreign value has no declared serialization schema.
pub const E_FOREIGN_TYPE_NO_SCHEMA: &str = "E_FOREIGN_TYPE_NO_SCHEMA";
/// Two non-adapter Type nodes both implement the same interface with overlapping
/// type families, creating a blanket impl coherence conflict.
pub const E_BLANKET_IMPL_OVERLAP: &str = "E_BLANKET_IMPL_OVERLAP";
/// A non-adapter implementation declares a foreign interface but neither the
/// interface name nor the implementing type name appears in the graph as an
/// owned node — orphan rule violation.
pub const E_ORPHAN_RULE_VIOLATION: &str = "E_ORPHAN_RULE_VIOLATION";
/// An EffectParam instantiation binding specifies an effect not present in
/// the caller's effect_row — the caller cannot supply that effect.
pub const E_EFFECT_PARAM_NOT_THREADED: &str = "E_EFFECT_PARAM_NOT_THREADED";
/// A CapabilityParam instantiation binding specifies a capability not present
/// in the caller's capability_reqs — the caller cannot supply that capability.
pub const E_CAPABILITY_PARAM_NOT_THREADED: &str = "E_CAPABILITY_PARAM_NOT_THREADED";
/// ConstParam instantiation value is not a decidable literal (not a simple
/// numeric string or simple identifier).
pub const E_CONST_PARAM_VALUE_INVALID: &str = "E_CONST_PARAM_VALUE_INVALID";

// ── Collection constraint table ───────────────────────────────────────────

/// Which constraints are required on the first type parameter of each
/// standard collection type.
///
/// `(nominal, requires_eq, requires_hash, requires_ord)`
pub(crate) const COLLECTION_CONSTRAINTS: &[(&str, bool, bool, bool)] = &[
    ("Set", true, true, false),
    ("Map", true, true, false),
    ("OrderedSet", true, false, true),
    ("OrderedMap", true, false, true),
];