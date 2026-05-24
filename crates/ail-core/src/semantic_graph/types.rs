// ── ail-core::semantic_graph::types ──────────────────────────────────────
//
// All data type definitions for the semantic graph: node/edge kinds, value
// types, metadata structs, and the core `GraphNode`, `GraphEdge`, and
// `SemanticGraph` structs.
//
// This module is private to `semantic_graph`; all public items are
// re-exported from `semantic_graph/mod.rs`.

use serde::{Deserialize, Serialize};

// ── NodeRef ───────────────────────────────────────────────────────────────

/// Opaque intra-graph identity for a `GraphNode`.
///
/// Scoped to one `SemanticGraph`; must not be used as a storage key.
/// Implements `Ord`/`PartialOrd` so that validation logic can use
/// ordered sets without requiring `HashMap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeRef(pub u32);

// ── NodeKind ──────────────────────────────────────────────────────────────

/// The semantic category of a `GraphNode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    /// A named module boundary.
    Module,
    /// A callable function.
    Function,
    /// A named type definition.
    Type,
    /// An effect declaration.
    Effect,
    /// A capability declaration.
    Capability,
    /// A contract definition.
    Contract,
    /// An invariant assertion.
    Invariant,
    /// A test node.
    Test,
    /// An architectural boundary marker.
    Boundary,
    /// A package boundary in the semantic graph.
    ///
    /// Added in Phase 12 (packages-trust-model) as an additive variant.
    /// Existing CBOR fixtures that do not use `Package` are unaffected
    /// because `ciborium` encodes enum variants by name string.
    Package,

    // ── ola3-core-ir-types: interface/impl/effect-alias kinds ────────────
    /// An interface (typeclass) definition.
    ///
    /// Represents a named set of method signatures and optional associated
    /// types that a type may implement.
    Interface,
    /// An implementation of an interface for a concrete type.
    ///
    /// Pairs with a corresponding `Interface` node via a `DependsOn` edge.
    Impl,
    /// An effect alias definition.
    ///
    /// Names a set of effects under a single alias so functions can
    /// declare the alias rather than enumerating individual effects.
    EffectAlias,

    // ── doc-alignment: module/package node kinds from §14 ─────────────────
    /// An import declaration node.
    ///
    /// Represents a semantic import of types, functions, or capabilities
    /// from another module or package.
    Import,
    /// An export declaration node.
    ///
    /// Represents a semantic export of types, functions, or capabilities
    /// from the current module or package.
    Export,
    /// A version constraint on a package dependency.
    VersionConstraint,
    /// A capability export declaration.
    ///
    /// Declares that a package exports a named capability.
    CapabilityExport,
    /// A contract export declaration.
    ///
    /// Declares that a package exports a named contract.
    ContractExport,
}

// ── EdgeKind ──────────────────────────────────────────────────────────────

/// The semantic relationship expressed by a `GraphEdge`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    /// Caller → callee dependency (static dispatch).
    Calls,
    /// Caller → callee dependency via dynamic dispatch (`Dyn<Interface>`).
    ///
    /// Distinguishes calls that resolve through an interface at runtime from
    /// static `Calls` edges.  Dynamic dispatch entries expose the interface
    /// contract and possible impls per runtime profile.
    DynCalls,
    /// Reader → data dependency.
    Reads,
    /// Writer → data dependency.
    Writes,
    /// Emitter → effect dependency.
    Emits,
    /// General module-level dependency.
    DependsOn,
    /// Proof obligation edge.
    Proves,
    /// Change-impact edge.
    BreaksIfChanged,
    /// Resource consumption edge (node consumes a resource).
    Consumes,
    /// Resource release edge (node releases a resource).
    Releases,
    /// Task is spawned by a task group.
    SpawnedBy,
    /// Child-of relationship (structural containment).
    ChildOf,
    /// Safe concurrency capability edge.
    SafeCapability,
}

// ── Storage identity value types ─────────────────────────────────────────

/// A content-addressed hash identifying the binary object stored for a
/// `GraphNode`.
///
/// Carries the raw hex digest (e.g., a BLAKE3 hex string) as produced by the
/// storage layer.  The string is opaque to `ail-core`; storage is responsible
/// for producing and validating it.
///
/// Uses `String` rather than a fixed-size byte array so that the CBOR
/// encoding remains schema-version-agnostic and readable in debug output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentHash {
    /// Hex-encoded content digest (e.g., BLAKE3).
    pub hex: String,
}

/// The provenance of a `GraphNode` — which `ChangeSet` or operation created
/// or last modified it.
///
/// The value is an opaque identifier (e.g., `"change.add_checkout"`) as
/// defined by the storage and change-management layers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Identifier of the originating change or operation.
    pub change_id: String,
}

/// The schema version under which a `GraphNode` was created.
///
/// Carries the schema name and version string (e.g., `"core_ir/2"`) so that
/// readers can apply the appropriate migrator if the current schema is newer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaRef {
    /// Schema identifier and version, e.g., `"core_ir/2"`.
    pub version: String,
}

/// The four canonical trust levels defined in `docs/core-ir.md §1/§13`.
///
/// A `Custom` variant is provided for backward-compatible classification tags
/// (e.g., `"resource:linear"`, `"task"`) that are not one of the four core
/// trust levels.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustLevel {
    /// Code/model proven within the system.
    Verified,
    /// External contract assumed without mechanical proof.
    Assumed,
    /// Not proven but isolated.
    Unverified,
    /// May break guarantees; requires explicit permission.
    Unsafe,
    /// Non-standard classification tag for backward compatibility.
    Custom(String),
}

impl TrustLevel {
    /// Return the string representation for downstream code that needs
    /// string-based matching.
    pub fn as_str(&self) -> &str {
        match self {
            TrustLevel::Verified => "verified",
            TrustLevel::Assumed => "assumed",
            TrustLevel::Unverified => "unverified",
            TrustLevel::Unsafe => "unsafe",
            TrustLevel::Custom(s) => s.as_str(),
        }
    }

    /// Parse a string into a `TrustLevel`, mapping the four canonical values
    /// to their typed variants and everything else to `Custom`.
    pub fn from_str_compat(s: &str) -> Self {
        match s {
            "verified" => TrustLevel::Verified,
            "assumed" => TrustLevel::Assumed,
            "unverified" => TrustLevel::Unverified,
            "unsafe" => TrustLevel::Unsafe,
            other => TrustLevel::Custom(other.to_string()),
        }
    }
}

/// Trust metadata associated with a `GraphNode`.
///
/// Records the trust level assigned by the policy layer and an optional
/// comment string.  The `level` field is a typed `TrustLevel` enum per
/// `docs/core-ir.md §1/§13`: `verified | assumed | unverified | unsafe`.
///
/// Uses `Vec<String>` for `tags` (not a `HashMap`) to guarantee deterministic
/// CBOR serialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustMetadata {
    /// Trust level — one of the four canonical levels or a custom classification.
    pub level: TrustLevel,
    /// Ordered tags qualifying the trust level (e.g., `["signed", "reviewed"]`).
    pub tags: Vec<String>,
}

// ── Span (source location) ────────────────────────────────────────────────

/// Optional reference to a textual view or source location for a `GraphNode`.
///
/// Corresponds to `docs/core-ir.md §1 — Span/View`.  Carries the source
/// file (or view name) and byte offsets so tooling can map IR nodes back to
/// the generated or authored text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// Source file or view identifier (e.g., `"src/checkout.ail"`).
    pub source: String,
    /// Byte offset of the start of the span (inclusive).
    pub start: u32,
    /// Byte offset of the end of the span (exclusive).
    pub end: u32,
}

// ── Semantic fact value types ─────────────────────────────────────────────

/// Contract clauses attached to a `GraphNode`.
///
/// Uses `Vec<String>` (no `HashMap`) for deterministic CBOR serialization.
/// Both lists are in declaration order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractClauses {
    /// Preconditions that callers must satisfy (e.g., `"x > 0"`).
    pub requires: Vec<String>,
    /// Postconditions the implementation guarantees (e.g., `"result >= 0"`).
    pub ensures: Vec<String>,
}

/// Materialized metadata for one runtime-check assertion on a `GraphNode`.
///
/// Does NOT execute anything; stores the predicate text and a stable content
/// hash so that tooling can track check identity across revisions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCheckMeta {
    /// The predicate expression, as a string (e.g., `"x != null"`).
    pub predicate: String,
    /// A stable content hash identifying this predicate (e.g., a hex digest).
    pub hash: String,
}

/// Resolved type information for a `GraphNode`.
///
/// Uses only `Vec<String>` (no `HashMap`) to guarantee deterministic CBOR
/// serialization with `ciborium`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeFacts {
    /// The nominal type name (e.g., `"Int"`, `"Bool"`, `"Map"`).
    pub nominal: String,
    /// Type parameters, in declaration order (e.g., `["Key", "Value"]`).
    pub generics: Vec<String>,
}

// ── Generic parameter kinds ───────────────────────────────────────────────

/// Classification of a generic parameter, as specified in docs/type-system.md.
///
/// The four kinds correspond to the four classes of generic parameters
/// supported by the type system: type, effect, capability, and const.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenericParamKind {
    /// A classical type parameter (e.g., `T` in `List<T>`).
    TypeParam,
    /// An effect parameter (e.g., `e` in `fn map<T, U, e>(...) effects e`).
    EffectParam,
    /// A capability parameter (e.g., `cap` in `fn with_retry<T, E, cap>(...)`).
    CapabilityParam,
    /// A decidable/simple const parameter (e.g., `N` in `Vector<T, N>`).
    ConstParam,
}

/// A first-class type-level constraint on a generic parameter.
///
/// Corresponds to `docs/core-ir.md §15/§16 — WhereConstraint`.
/// Instead of bare strings, each constraint carries structured information
/// about the interface requirement, the target parameter, and optionally
/// any associated type bindings required by the constraint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhereConstraint {
    /// The interface or trait name required (e.g., `"Eq"`, `"Hashable"`).
    pub interface: String,
    /// The type parameter this constraint applies to (e.g., `"T"`, `"K"`).
    ///
    /// When the constraint is stored inline in a `GenericParamDecl`, this
    /// is typically the same as the declaring parameter's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_param: Option<String>,
    /// Associated type bindings required by this constraint (e.g.,
    /// `[("Error", "DbError")]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub associated_types: Vec<AssociatedTypeBinding>,
}

/// A typed generic parameter declaration on a function or type node.
///
/// Each entry names a generic parameter and classifies its kind.
/// `required_constraints` carries structured `WhereConstraint` entries
/// per `docs/core-ir.md §16`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenericParamDecl {
    /// Parameter name (e.g., `"T"`, `"e"`, `"N"`).
    pub name: String,
    /// Parameter kind.
    pub kind: GenericParamKind,
    /// Structured interface constraints required on this parameter.
    ///
    /// Each entry is a typed `WhereConstraint` instead of a bare string.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_constraints: Vec<WhereConstraint>,
}

// ── Function parameter declarations ──────────────────────────────────────

/// A declared function parameter with its nominal type name.
///
/// Used to record the expected parameter types of a function node,
/// enabling call-site nominal type enforcement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamDecl {
    /// Parameter name (e.g., `"id"`, `"value"`).
    pub name: String,
    /// Declared nominal type (e.g., `"UserId"`, `"OrderId"`, `"Int"`).
    pub ty: String,
}

// ── Interface metadata ────────────────────────────────────────────────────

/// An associated type binding in an interface implementation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssociatedTypeBinding {
    /// Associated type name declared in the interface (e.g., `"Error"`).
    pub name: String,
    /// Concrete type bound at this implementation site (e.g., `"DbError"`).
    pub ty: String,
}

/// Interface implementation metadata declared on a node.
///
/// Records that the owning node's type implements the named interface,
/// with optional associated type bindings and an adapter flag for the
/// orphan rule exception.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceImplMeta {
    /// Fully-qualified interface name (e.g., `"cap.payments.Chargeable"`).
    pub interface: String,
    /// Associated type bindings for this impl, in declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub associated_types: Vec<AssociatedTypeBinding>,
    /// Whether this implementation uses the adapter/newtype exception.
    ///
    /// When `true`, the orphan rule exception applies.
    pub is_adapter: bool,
}

// ── Refinement types ──────────────────────────────────────────────────────

/// Possible outcomes for a refinement proof obligation.
///
/// Mirrors the six verification states but restricted to refinement context.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefinementStatus {
    /// Statically proven by the type checker or solver.
    Proven,
    /// Validated by a runtime check that is materialized at an explicit boundary.
    RuntimeChecked,
    /// Declared/assumed by the programmer without mechanical proof.
    Assumed,
    /// Not yet classified; outcome unknown.
    Unverified,
    /// The refinement is known to fail (violation detected).
    Failed,
}

/// A refinement predicate attached to a type node.
///
/// The `status` field carries the pre-classified proof obligation outcome.
/// `erased` records whether this refinement was downgraded to the base type
/// at some boundary — erasure must always be explicit and tracked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefinementRef {
    /// The base type being refined (e.g., `"Text"`, `"Int"`).
    pub base_type: String,
    /// The predicate expression (e.g., `"length_graphemes(value) > 0"`).
    pub predicate: String,
    /// Classification of the proof obligation.
    pub status: RefinementStatus,
    /// Whether this refinement was erased to the base type at a boundary.
    ///
    /// When `true`, an erasure entry is emitted in the verification report.
    #[serde(default)]
    pub erased: bool,
}

// ── Constraint set ────────────────────────────────────────────────────────

/// Explicit constraint declarations on a type node.
///
/// Records whether this type implements `Eq`, `Ord`, `PartialOrd`, and `Hashable`,
/// plus any additional named constraints.  These are used by the type
/// checker to enforce collection/operator constraint requirements.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstraintSet {
    /// Whether this type implements `Eq` (required for `==`, `Set<T>`, `Map<K,V>`).
    pub has_eq: bool,
    /// Whether this type implements `Ord` (total order; required for `sort`, `min`, `max`).
    pub has_ord: bool,
    /// Whether this type implements `Hashable` (required for `Set<T>`, `Map<K,V>`).
    pub has_hash: bool,
    /// Whether this type implements `PartialOrd` (partial order, e.g., for floating-point).
    ///
    /// Distinct from `has_ord`: a type may have partial order without total order.
    /// Serialized only when `true`; defaults to `false` for backward compatibility
    /// with existing CBOR fixtures that omit this field.
    #[serde(default)]
    pub has_partial_ord: bool,
    /// Additional named constraints (e.g., `["Display"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extras: Vec<String>,
}

// ── Call-site type argument bindings ─────────────────────────────────────

/// A binding from a generic parameter name to the concrete type at a call site.
///
/// Used on `GraphEdge` (kind `Calls`) to record which concrete type fills
/// each generic type parameter at this invocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeArgBinding {
    /// Generic parameter name (e.g., `"T"`, `"K"`, `"V"`).
    pub param: String,
    /// Concrete type filling this parameter (e.g., `"UserId"`, `"Int"`).
    pub ty: String,
}

// ── Effect and capability argument bindings ───────────────────────────────
//
// Added in ola4-type-formalism to model the instantiation of EffectParam and
// CapabilityParam generic parameters at Calls edges.  Both structs are optional
// on `GraphEdge` (serde default + skip_serializing_if) for backward compat.

/// Instantiation binding for an EffectParam at a `Calls` edge.
///
/// Carries the name of the effect parameter (`param`) and the concrete list of
/// effects that must be supplied by the caller at this call site.
/// Symmetric with `TypeArgBinding`; both live on `GraphEdge.type_arg_bindings`
/// and `GraphEdge.effect_arg_bindings` respectively.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectArgBinding {
    /// Effect parameter name declared on the callee (e.g., `"e"`).
    pub param: String,
    /// Concrete effects the caller must supply (e.g., `["IO"]`).
    pub effects: Vec<String>,
}

/// Instantiation binding for a CapabilityParam at a `Calls` edge.
///
/// Carries the name of the capability parameter (`param`) and the concrete
/// list of capability names that must be present in the caller's capability
/// requirements at this call site.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityArgBinding {
    /// Capability parameter name declared on the callee (e.g., `"cap"`).
    pub param: String,
    /// Concrete capabilities the caller must supply (e.g., `["net:read"]`).
    pub caps: Vec<String>,
}

/// Declared effect row for a `GraphNode`.
///
/// Uses `Vec<String>` (no `HashMap`) for CBOR determinism.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectRow {
    /// Named effects declared on this node (e.g., `["IO", "State"]`).
    pub effects: Vec<String>,
}

/// Declared capability requirements for a `GraphNode`.
///
/// Uses `Vec<String>` (no `HashMap`) for CBOR determinism.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReqs {
    /// Named capability requirements (e.g., `["net:read", "fs:write"]`).
    pub caps: Vec<String>,
}

// ── Change-language representation metadata ───────────────────────────────

/// Export visibility attached to a graph node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    /// The node is exported as public API.
    Public,
    /// The node is not exported.
    #[default]
    Private,
    /// The node is visible only inside an implementation boundary.
    Internal,
}

/// Binding from a semantic name to an implementation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// Bound capability, name, or interface.
    pub name: String,
    /// Concrete implementation or handler.
    pub implementation: String,
    /// Optional runtime profile for the binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

/// Materialized inference attached to a graph node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferredFact {
    /// Inference category, such as `boundary`, `effects`, or `return`.
    pub kind: String,
    /// Inferred value or pending marker.
    pub value: String,
}

/// Controlled generated artifact reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedArtifact {
    /// Artifact category, such as `tests`, `sdk`, or `docs`.
    pub kind: String,
    /// Source or selector used to generate the artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Compile-time assertion attached to a graph node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assertion {
    /// Assertion category, such as `exists`, `signature`, or `hash`.
    pub kind: String,
    /// Assertion value, hash, or marker.
    pub value: String,
}

/// Semantic workflow state for a graph node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowState {
    /// Default editable state.
    #[default]
    Draft,
    /// API, behavior, or contracts are locked.
    Locked,
    /// Proposal, inference, or assumption accepted.
    Approved,
    /// Proposal, inference, or assumption rejected.
    Rejected,
    /// Node has been verified.
    Verified,
    /// Node is undergoing an intentional migration.
    Migrating,
    /// Node is undergoing behavior-preserving refactoring.
    Refactoring,
}

// ── HandlerMeta ───────────────────────────────────────────────────────────

/// Metadata for a handler node — a function that satisfies one or more
/// capability contracts.
///
/// Carried as an optional field on `GraphNode` so that existing fixtures
/// without handler metadata are unaffected (backward compatible).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandlerMeta {
    /// Capabilities this handler intercepts (e.g., `["database.read"]`).
    pub handled_caps: Vec<String>,
    /// Internal effects emitted by this handler's implementation body.
    pub internal_effects: Vec<String>,
    /// The capability contract this handler claims to satisfy, if any.
    ///
    /// Serialized only when `Some`; absent when the handler does not
    /// claim a specific contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub satisfies_contract: Option<String>,
}

// ── GraphNode ─────────────────────────────────────────────────────────────

/// A typed node in the semantic graph.
///
/// # Backward Compatibility
///
/// All optional fields are serialized only when `Some` and deserialize as
/// `None` when absent.  This keeps every prior CBOR wire format byte-identical
/// when the corresponding fields are not populated.  Storage identity fields
/// (`content_hash`, `provenance`, `schema`, `trust_metadata`) were added in
/// G15 following the same pattern.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    /// Intra-graph identity.
    pub id: NodeRef,
    /// Semantic category of this node.
    pub kind: NodeKind,
    /// Human-readable name (e.g., fully qualified symbol name).
    pub name: String,
    /// Resolved type information, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_facts: Option<TypeFacts>,
    /// Declared effect row, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_row: Option<EffectRow>,
    /// Declared capability requirements, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_reqs: Option<CapabilityReqs>,
    /// Contract clauses (requires/ensures), if declared.
    ///
    /// Serialized only when `Some`; absent fields deserialize as `None`.
    /// This keeps Phase 1–5 CBOR wire format byte-identical when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_clauses: Option<ContractClauses>,
    /// Materialized runtime-check metadata, if any checks are registered.
    ///
    /// Serialized only when `Some`; absent fields deserialize as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_checks: Option<Vec<RuntimeCheckMeta>>,
    /// Content-addressed hash of the stored binary object for this node.
    ///
    /// Populated by the storage layer after the node is persisted.
    /// `None` for in-memory nodes that have not yet been committed to storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<ContentHash>,
    /// Provenance: which change or operation last created/modified this node.
    ///
    /// Set by the change-management layer when a `ChangeSet` is applied.
    /// `None` for nodes not yet associated with a committed change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    /// Schema version under which this node was created.
    ///
    /// Used by the storage layer to select the appropriate migrator when
    /// reading older objects.  `None` for nodes created without explicit
    /// schema tracking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<SchemaRef>,
    /// Trust metadata assigned by the policy layer.
    ///
    /// `None` for nodes that have not yet passed through the trust-assignment
    /// pipeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_metadata: Option<TrustMetadata>,

    // ── Type-system enforcement metadata (G24) ────────────────────────────
    //
    // All fields below are additive and optional.  Existing CBOR fixtures that
    // omit them are unaffected — fields absent in the wire format deserialize
    // as `None`.  The `GraphNode::new` constructor initialises all to `None`.
    /// Typed generic parameter declarations for this node.
    ///
    /// Present on Function/Type nodes that declare generic parameters.
    /// Carries kind information (`TypeParam`, `EffectParam`, etc.) and
    /// any required interface constraints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generic_params: Option<Vec<GenericParamDecl>>,

    /// Declared function parameters with their expected nominal types.
    ///
    /// Present on `Function` nodes.  Used by the type checker to enforce
    /// nominal type compatibility at call sites.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<ParamDecl>>,

    /// Declared return type of this function (nominal type name).
    ///
    /// Present on `Function` nodes with an explicit return type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,

    /// ACL expression body text for a function node.
    ///
    /// Parsed by the compiler into `CoreExpr` during lowering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_expr: Option<String>,

    /// Interface implementations declared by this node's type.
    ///
    /// Present on `Type` (or `Function`) nodes that implement interfaces.
    /// Used for coherence/orphan-rule checking and Dyn<Interface> coverage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface_impls: Option<Vec<InterfaceImplMeta>>,

    /// Refinement predicate for this type, with its proof-obligation status.
    ///
    /// Present on `Type` nodes that carry a refinement constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refinement_ref: Option<RefinementRef>,

    /// Explicit constraint declarations (Eq/Ord/Hashable/extras).
    ///
    /// Present on `Type` nodes to record which constraints they satisfy.
    /// Checked by the type checker when verifying generic instantiations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraint_set: Option<ConstraintSet>,

    /// Export visibility requested through `expose` / `hide` ops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,

    /// Handler/name bindings attached through `bind` ops.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<Binding>,

    /// Inferred facts attached through `infer` ops.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inferred: Vec<InferredFact>,

    /// Derived implementation names attached through `derive` ops.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_impls: Vec<String>,

    /// Generated artifact references attached through `generate` ops.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_artifacts: Vec<GeneratedArtifact>,

    /// Compile-time assertions attached through `assert` ops.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<Assertion>,

    /// Workflow state attached through workflow ops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_state: Option<WorkflowState>,

    /// Handler metadata — present on nodes that act as capability handlers.
    ///
    /// `None` for all other node kinds.  Serialized only when `Some`
    /// so that existing CBOR fixtures are byte-identical when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handler_meta: Option<HandlerMeta>,

    // ── doc-alignment: identity fields from docs/core-ir.md §1 ────────────
    /// Optional source location / textual view reference.
    ///
    /// Corresponds to `docs/core-ir.md §1 — Span/View`.
    /// `None` for nodes without a known source location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,

    /// Human-readable stable identifier (e.g., `"fn.cart_total"`,
    /// `"type.Money"`, `"cap.payment.charge"`).
    ///
    /// Corresponds to `docs/core-ir.md §1 — NodeId`.  Coexists with
    /// `NodeRef(u32)` which remains the efficient intra-graph index.
    /// The `stable_id` is the external-facing, content-addressable identity
    /// that survives refactors and graph rebuilds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_id: Option<String>,
}

impl GraphNode {
    /// Create a new `GraphNode` with all optional fields set to `None`.
    ///
    /// This is the preferred constructor for all phases and for new nodes that
    /// do not yet have resolved type/effect/capability or storage identity
    /// information.  Using this constructor avoids source-compat breaks when
    /// additional optional fields are added.
    ///
    /// Storage identity fields (`content_hash`, `provenance`, `schema`,
    /// `trust_metadata`) are also initialized to `None`; the storage and
    /// change-management layers populate them after persistence.
    ///
    /// # Example
    ///
    /// ```rust
    /// use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef};
    ///
    /// let node = GraphNode::new(NodeRef(0), NodeKind::Module, "core");
    /// assert_eq!(node.name, "core");
    /// assert!(node.type_facts.is_none());
    /// assert!(node.content_hash.is_none());
    /// ```
    pub fn new(id: NodeRef, kind: NodeKind, name: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            name: name.into(),
            type_facts: None,
            effect_row: None,
            capability_reqs: None,
            contract_clauses: None,
            runtime_checks: None,
            content_hash: None,
            provenance: None,
            schema: None,
            trust_metadata: None,
            generic_params: None,
            params: None,
            return_type: None,
            body_expr: None,
            interface_impls: None,
            refinement_ref: None,
            constraint_set: None,
            visibility: None,
            bindings: vec![],
            inferred: vec![],
            derived_impls: vec![],
            generated_artifacts: vec![],
            assertions: vec![],
            workflow_state: None,
            handler_meta: None,
            span: None,
            stable_id: None,
        }
    }
}

// ── GraphEdge ─────────────────────────────────────────────────────────────

/// A directed, typed edge between two `GraphNode`s.
///
/// The optional `call_args`, `type_arg_bindings`, `effect_arg_bindings`, and
/// `capability_arg_bindings` fields are populated only on `Calls` edges and are
/// absent from the wire format otherwise, preserving backward CBOR compatibility.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Source node.
    pub source: NodeRef,
    /// Target node.
    pub target: NodeRef,
    /// Semantic relationship.
    pub kind: EdgeKind,
    /// Argument types at this call site, in parameter order.
    ///
    /// Present only on `Calls` edges when argument types are known.
    /// Each string is the nominal type name of the corresponding argument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_args: Option<Vec<String>>,
    /// Generic type argument bindings at this call site.
    ///
    /// Present only on `Calls` edges where generic parameters are
    /// instantiated with concrete types.  Used by the constraint checker
    /// to verify that concrete types satisfy the callee's generic constraints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_arg_bindings: Option<Vec<TypeArgBinding>>,

    // ── ola4-type-formalism: effect and capability param threading ────────
    /// Effect argument bindings at this call site.
    ///
    /// Present only on `Calls` edges where EffectParam generic parameters are
    /// instantiated.  Each binding maps a parameter name to the concrete list
    /// of effects the caller supplies.  Absent from the wire format when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_arg_bindings: Option<Vec<EffectArgBinding>>,
    /// Capability argument bindings at this call site.
    ///
    /// Present only on `Calls` edges where CapabilityParam generic parameters
    /// are instantiated.  Each binding maps a parameter name to the concrete
    /// list of capabilities the caller supplies.  Absent from the wire format
    /// when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_arg_bindings: Option<Vec<CapabilityArgBinding>>,
}

impl GraphEdge {
    /// Create a simple edge with no call-site metadata.
    pub fn new(source: NodeRef, target: NodeRef, kind: EdgeKind) -> Self {
        Self {
            source,
            target,
            kind,
            call_args: None,
            type_arg_bindings: None,
            effect_arg_bindings: None,
            capability_arg_bindings: None,
        }
    }
}

// ── SemanticGraph ─────────────────────────────────────────────────────────

/// The canonical program representation as a typed directed graph.
///
/// Uses `Vec` for both `nodes` and `edges` to guarantee deterministic CBOR
/// serialization.  Validation logic builds transient ordered sets internally.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticGraph {
    /// All nodes in the graph, in insertion order.
    pub nodes: Vec<GraphNode>,
    /// All edges in the graph, in insertion order.
    pub edges: Vec<GraphEdge>,
}
