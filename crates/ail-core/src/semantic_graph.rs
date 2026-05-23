// ── ail-core::semantic_graph ──────────────────────────────────────────────
//
// Canonical typed graph representation for the AIL program model.
//
// # Identity contract
//
// `NodeRef(u32)` is the intra-graph identity for nodes within one
// `SemanticGraph`.  It is NOT a storage identity; that role belongs to
// `ail_storage::object::ObjectId`.  A `NodeRef` must never cross the storage
// boundary.
//
// # Determinism contract
//
// All serializable fields use `Vec` or `BTreeMap` — never `HashMap` — to
// guarantee CBOR output determinism with `ciborium`.  Validation helpers may
// build transient `BTreeSet` / `BTreeMap` structures internally, but those
// collections are never part of the serialized layout.

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

// ── GraphValidationError ──────────────────────────────────────────────────

/// Errors produced by `SemanticGraph::validate` and `SemanticGraph::validate_full`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphValidationError {
    /// Two nodes share the same `NodeRef`.
    DuplicateRef(NodeRef),
    /// An edge endpoint references a `NodeRef` not present in the graph.
    DanglingEdge {
        /// The missing `NodeRef`.
        r#ref: NodeRef,
        /// Whether the missing ref was the edge source or target.
        role: DanglingRole,
    },
    /// A node declares a non-empty `effect_row` but has no outgoing `Emits` edges.
    ///
    /// Emitting effects requires graph edges — a declared effect row that is
    /// never wired to an `Emits` edge is an incoherent graph state.
    EffectRowNoEmitsEdge(NodeRef),
    /// A node's `capability_reqs` names a capability that has no matching
    /// `Capability`-kind node in this graph.
    CapabilityReqsMissingNode {
        /// The node that declared the unsatisfied requirement.
        owner_ref: NodeRef,
        /// The capability name that could not be matched to any `Capability` node.
        cap_name: String,
    },
}

/// Whether a dangling edge endpoint was the source or the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DanglingRole {
    Source,
    Target,
}

// ── SemanticGraph::validate ───────────────────────────────────────────────

impl SemanticGraph {
    /// Validate structural invariants:
    ///
    /// 1. All `NodeRef`s in `nodes` are unique.
    /// 2. Every edge endpoint corresponds to an existing node in this graph.
    ///
    /// Returns `Ok(())` when all invariants hold; otherwise returns the first
    /// `GraphValidationError` found.
    pub fn validate(&self) -> Result<(), GraphValidationError> {
        use std::collections::BTreeSet;

        // Pass 1 — build the set of known refs, detecting duplicates.
        let mut seen: BTreeSet<NodeRef> = BTreeSet::new();
        for node in &self.nodes {
            if !seen.insert(node.id) {
                return Err(GraphValidationError::DuplicateRef(node.id));
            }
        }

        // Pass 2 — verify all edge endpoints are in the known set.
        for edge in &self.edges {
            if !seen.contains(&edge.source) {
                return Err(GraphValidationError::DanglingEdge {
                    r#ref: edge.source,
                    role: DanglingRole::Source,
                });
            }
            if !seen.contains(&edge.target) {
                return Err(GraphValidationError::DanglingEdge {
                    r#ref: edge.target,
                    role: DanglingRole::Target,
                });
            }
        }

        Ok(())
    }

    /// Full semantic validation — returns ALL errors found (not just the first).
    ///
    /// Performs the same structural checks as [`validate`] (duplicate refs,
    /// dangling edges) plus two additional semantic coherence checks:
    ///
    /// 3. **Effect-row coherence** — every node whose `effect_row` is `Some` and
    ///    non-empty must have at least one outgoing `Emits` edge.  A declared
    ///    effect row that is never connected to an `Emits` edge is incoherent.
    ///
    /// 4. **Capability-reqs consistency** — every capability name listed in a
    ///    node's `capability_reqs` must correspond to a `Capability`-kind node
    ///    present in this graph.  Requirements that reference non-existent
    ///    capability nodes indicate a malformed graph.
    ///
    /// Returns an empty `Vec` when all invariants hold; the caller can call
    /// `validate_full().is_empty()` to test overall validity.
    pub fn validate_full(&self) -> Vec<GraphValidationError> {
        use std::collections::BTreeSet;

        let mut errors: Vec<GraphValidationError> = Vec::new();

        // Pass 1 — duplicate NodeRef detection.
        let mut seen: BTreeSet<NodeRef> = BTreeSet::new();
        for node in &self.nodes {
            if !seen.insert(node.id) {
                errors.push(GraphValidationError::DuplicateRef(node.id));
            }
        }

        // Pass 2 — dangling edge endpoints.
        for edge in &self.edges {
            if !seen.contains(&edge.source) {
                errors.push(GraphValidationError::DanglingEdge {
                    r#ref: edge.source,
                    role: DanglingRole::Source,
                });
            }
            if !seen.contains(&edge.target) {
                errors.push(GraphValidationError::DanglingEdge {
                    r#ref: edge.target,
                    role: DanglingRole::Target,
                });
            }
        }

        // Pass 3 — effect-row coherence.
        // A node with a non-empty effect_row must have at least one Emits edge.
        for node in &self.nodes {
            if node
                .effect_row
                .as_ref()
                .is_some_and(|r| !r.effects.is_empty())
            {
                let has_emits = self
                    .edges
                    .iter()
                    .any(|e| e.source == node.id && e.kind == EdgeKind::Emits);
                if !has_emits {
                    errors.push(GraphValidationError::EffectRowNoEmitsEdge(node.id));
                }
            }
        }

        // Pass 4 — capability-reqs consistency.
        // Build the set of Capability-kind node names available in this graph.
        let capability_names: BTreeSet<&str> = self
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Capability)
            .map(|n| n.name.as_str())
            .collect();

        for node in &self.nodes {
            if let Some(cap_reqs) = &node.capability_reqs {
                for cap_name in &cap_reqs.caps {
                    if !capability_names.contains(cap_name.as_str()) {
                        errors.push(GraphValidationError::CapabilityReqsMissingNode {
                            owner_ref: node.id,
                            cap_name: cap_name.clone(),
                        });
                    }
                }
            }
        }

        errors
    }
}

// ── Semantic ref newtypes ─────────────────────────────────────────────────

/// Typed identity for a block in the semantic graph.
///
/// Wraps an opaque string identifier (e.g., `"block_checkout_flow"`).
/// Using `#[serde(transparent)]` keeps the CBOR wire format identical to a
/// plain `String` for backward compatibility.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockRef(pub String);

/// Typed identity for a contract associated with a semantic node.
///
/// Wraps an opaque string identifier (e.g., `"contract.checkout.payment"`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContractRef(pub String);

/// Typed identity for an effect associated with a semantic node.
///
/// Wraps an opaque string identifier (e.g., `"effect.database.read"`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EffectRef(pub String);

/// Typed identity for a proof obligation attached to a semantic node.
///
/// Wraps an opaque string identifier (e.g., `"proof.invariant.no_negative_balance"`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProofObligationRef(pub String);

/// Typed identity for a runtime-check assertion at a semantic node.
///
/// Wraps an opaque string identifier (e.g., `"rtcheck.null_guard.cart_id"`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeCheckRef(pub String);

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ───────────────────────────────────────────────────────────

    fn node(id: u32, kind: NodeKind, name: &str) -> GraphNode {
        GraphNode::new(NodeRef(id), kind, name)
    }

    fn edge(source: u32, target: u32, kind: EdgeKind) -> GraphEdge {
        GraphEdge::new(NodeRef(source), NodeRef(target), kind)
    }

    // ── valid_graph_passes_validation ─────────────────────────────────────
    // Spec scenario: "Unique refs pass validation"
    //   GIVEN a graph with nodes NodeRef(0), NodeRef(1), NodeRef(2)
    //   WHEN validate() is called
    //   THEN validation returns Ok(())
    //
    // RED: written first — types exist now, validate() stubs returning Ok(())
    // GREEN: will pass with real implementation
    #[test]
    fn valid_graph_passes_validation() {
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "core"),
                node(1, NodeKind::Function, "run"),
                node(2, NodeKind::Type, "Config"),
            ],
            edges: vec![edge(0, 1, EdgeKind::DependsOn), edge(1, 2, EdgeKind::Reads)],
        };
        assert_eq!(graph.validate(), Ok(()));
    }

    // ── duplicate_node_ref_is_rejected ────────────────────────────────────
    // Spec scenario: "Duplicate NodeRef is rejected"
    //   GIVEN a graph builder that inserts two nodes both with NodeRef(0)
    //   WHEN validate() is called
    //   THEN validation returns Err identifying the duplicate ref
    #[test]
    fn duplicate_node_ref_is_rejected() {
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "a"),
                node(0, NodeKind::Function, "b"), // duplicate!
            ],
            edges: vec![],
        };
        assert_eq!(
            graph.validate(),
            Err(GraphValidationError::DuplicateRef(NodeRef(0)))
        );
    }

    // ── dangling_edge_source_is_rejected ──────────────────────────────────
    // Spec scenario: "Edge with missing source is rejected"
    //   GIVEN a graph containing NodeRef(1) but not NodeRef(99)
    //   WHEN an edge (NodeRef(99) → NodeRef(1)) is added and validate() called
    //   THEN validation returns Err naming the missing source ref
    #[test]
    fn dangling_edge_source_is_rejected() {
        let graph = SemanticGraph {
            nodes: vec![node(1, NodeKind::Function, "target_fn")],
            edges: vec![edge(99, 1, EdgeKind::Calls)], // source 99 is missing
        };
        assert_eq!(
            graph.validate(),
            Err(GraphValidationError::DanglingEdge {
                r#ref: NodeRef(99),
                role: DanglingRole::Source,
            })
        );
    }

    // ── dangling_edge_target_is_rejected ──────────────────────────────────
    // Spec scenario: "Edge with missing target"
    //   GIVEN a graph containing NodeRef(0) but not NodeRef(77)
    //   WHEN an edge (NodeRef(0) → NodeRef(77)) is added and validate() called
    //   THEN validation returns Err naming the missing target ref
    #[test]
    fn dangling_edge_target_is_rejected() {
        let graph = SemanticGraph {
            nodes: vec![node(0, NodeKind::Module, "source_mod")],
            edges: vec![edge(0, 77, EdgeKind::DependsOn)], // target 77 is missing
        };
        assert_eq!(
            graph.validate(),
            Err(GraphValidationError::DanglingEdge {
                r#ref: NodeRef(77),
                role: DanglingRole::Target,
            })
        );
    }

    // ── TRIANGULATE: edge_with_present_endpoints_passes ───────────────────
    // Spec scenario: "Edge with present endpoints passes"
    //   GIVEN a graph with NodeRef(0) and NodeRef(1)
    //   WHEN an edge (NodeRef(0) → NodeRef(1)) is added and validate() called
    //   THEN validation returns Ok(())
    //
    // Different from valid_graph_passes_validation: single edge, minimal setup.
    #[test]
    fn edge_with_present_endpoints_passes() {
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "src"),
                node(1, NodeKind::Module, "dst"),
            ],
            edges: vec![edge(0, 1, EdgeKind::DependsOn)],
        };
        assert_eq!(graph.validate(), Ok(()));
    }

    // ── TRIANGULATE: empty_graph_passes_validation ────────────────────────
    // Edge case: a graph with no nodes and no edges is structurally valid.
    #[test]
    fn empty_graph_passes_validation() {
        let graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };
        assert_eq!(graph.validate(), Ok(()));
    }

    // ── cbor_encodes_deterministically ────────────────────────────────────
    // Spec scenario: "Re-serialization produces identical bytes"
    //   GIVEN a SemanticGraph serialized to CBOR
    //   WHEN the bytes are deserialized and re-serialized
    //   THEN the output bytes are identical to the original
    //
    // Uses ail_storage::codec::CborCodec — added as dev-dependency.
    #[test]
    fn cbor_encodes_deterministically() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "mod_a"),
                node(1, NodeKind::Function, "fn_b"),
                node(2, NodeKind::Effect, "eff_c"),
            ],
            edges: vec![edge(0, 1, EdgeKind::DependsOn), edge(1, 2, EdgeKind::Emits)],
        };

        let bytes_a = codec.encode(&graph).expect("first encode must succeed");
        let bytes_b = codec.encode(&graph).expect("second encode must succeed");
        assert_eq!(
            bytes_a, bytes_b,
            "identical SemanticGraph inputs must produce identical CBOR bytes"
        );

        // TRIANGULATE: also verify re-deserialization produces the original.
        let decoded: SemanticGraph = codec.decode(&bytes_a).expect("decode must succeed");
        assert_eq!(
            decoded, graph,
            "decoded SemanticGraph must equal the original"
        );
    }

    // ── package_node_cbor_round_trip ──────────────────────────────────────
    // Spec scenario: "Package node round-trips through CBOR"
    //   GIVEN a GraphNode with kind: NodeKind::Package
    //   WHEN serialized to CBOR and deserialized
    //   THEN kind equals NodeKind::Package
    //
    // Also verifies the additive variant does not disturb existing node kinds.
    #[test]
    fn package_node_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "root"),
                node(1, NodeKind::Package, "payments.stripe"),
            ],
            edges: vec![edge(0, 1, EdgeKind::DependsOn)],
        };

        let bytes = codec.encode(&graph).expect("encode must succeed");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode must succeed");

        assert_eq!(
            decoded, graph,
            "graph with Package node must survive CBOR round-trip"
        );
        assert_eq!(
            decoded.nodes[1].kind,
            NodeKind::Package,
            "Package kind must be preserved"
        );
    }

    // ── G24: generic_params_cbor_round_trip ───────────────────────────────
    // Spec requirement 2 (Generics): GenericParamDecl with all kinds round-trips.
    //   GIVEN a Function node with generic_params covering all four kinds
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are preserved exactly
    //
    // RED: written before GenericParamDecl / GenericParamKind existed.
    // GREEN: passes after Task 1 implementation.
    #[test]
    fn generic_params_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "traverse");
        node.generic_params = Some(vec![
            GenericParamDecl {
                name: "T".into(),
                kind: GenericParamKind::TypeParam,
                required_constraints: vec![],
            },
            GenericParamDecl {
                name: "e".into(),
                kind: GenericParamKind::EffectParam,
                required_constraints: vec![],
            },
            GenericParamDecl {
                name: "cap".into(),
                kind: GenericParamKind::CapabilityParam,
                required_constraints: vec![],
            },
            GenericParamDecl {
                name: "N".into(),
                kind: GenericParamKind::ConstParam,
                required_constraints: vec![],
            },
        ]);

        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode must succeed");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(
            decoded, graph,
            "graph with generic_params must survive CBOR round-trip"
        );
        assert_eq!(
            decoded.nodes[0].generic_params.as_ref().unwrap().len(),
            4,
            "all four generic param declarations must be preserved"
        );
    }

    // ── G24: params_and_return_type_cbor_round_trip ───────────────────────
    // Spec requirement 1 (Nominal): Function params with explicit types round-trip.
    //   GIVEN a Function node with declared params and return_type
    //   WHEN serialized to CBOR and deserialized
    //   THEN all param declarations are preserved
    #[test]
    fn params_and_return_type_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "load_user");
        node.params = Some(vec![ParamDecl {
            name: "id".into(),
            ty: "UserId".into(),
        }]);
        node.return_type = Some("User".into());

        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let params = decoded.nodes[0].params.as_ref().unwrap();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "id");
        assert_eq!(params[0].ty, "UserId");
        assert_eq!(decoded.nodes[0].return_type.as_deref(), Some("User"));
    }

    // ── G24: interface_impl_meta_cbor_round_trip ──────────────────────────
    // Spec requirement 3 (Interfaces): InterfaceImplMeta with associated types.
    //   GIVEN a Type node with interface_impls including associated type bindings
    //   WHEN serialized to CBOR and deserialized
    //   THEN all impl data is preserved
    #[test]
    fn interface_impl_meta_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "PostgresUserRepo");
        node.interface_impls = Some(vec![InterfaceImplMeta {
            interface: "cap.Repository<User>".into(),
            associated_types: vec![AssociatedTypeBinding {
                name: "Error".into(),
                ty: "DbError".into(),
            }],
            is_adapter: false,
        }]);

        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let impls = decoded.nodes[0].interface_impls.as_ref().unwrap();
        assert_eq!(impls.len(), 1);
        assert_eq!(impls[0].interface, "cap.Repository<User>");
        assert_eq!(impls[0].associated_types[0].ty, "DbError");
        assert!(!impls[0].is_adapter);
    }

    // ── G24: refinement_ref_cbor_round_trip ──────────────────────────────
    // Spec requirement 6 (Refinements): RefinementRef with status and erasure flag.
    //   GIVEN a Type node with a refinement predicate and RuntimeChecked status
    //   WHEN serialized to CBOR and deserialized
    //   THEN all refinement fields are preserved
    #[test]
    fn refinement_ref_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "Email");
        node.refinement_ref = Some(RefinementRef {
            base_type: "Text".into(),
            predicate: "matches_email(value)".into(),
            status: RefinementStatus::RuntimeChecked,
            erased: false,
        });

        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let rf = decoded.nodes[0].refinement_ref.as_ref().unwrap();
        assert_eq!(rf.base_type, "Text");
        assert_eq!(rf.status, RefinementStatus::RuntimeChecked);
        assert!(!rf.erased);
    }

    // ── G24: constraint_set_cbor_round_trip ──────────────────────────────
    // Spec requirement 5 (Eq/Ord/Hash): ConstraintSet with all flags.
    //   GIVEN a Type node declaring Eq + Hash constraints
    //   WHEN serialized to CBOR and deserialized
    //   THEN constraint flags are preserved exactly
    #[test]
    fn constraint_set_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "UserId");
        node.constraint_set = Some(ConstraintSet {
            has_eq: true,
            has_ord: false,
            has_hash: true,
            has_partial_ord: false,
            extras: vec!["Display".into()],
        });

        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let cs = decoded.nodes[0].constraint_set.as_ref().unwrap();
        assert!(cs.has_eq);
        assert!(!cs.has_ord);
        assert!(cs.has_hash);
        assert_eq!(cs.extras, ["Display"]);
    }

    // ── G24: call_edge_with_args_cbor_round_trip ──────────────────────────
    // Spec requirement 1 (Nominal): Call edge with arg types and type bindings.
    //   GIVEN a Calls edge with call_args and type_arg_bindings
    //   WHEN serialized to CBOR and deserialized
    //   THEN all call-site metadata is preserved
    #[test]
    fn call_edge_with_args_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Function, "caller"),
                node(1, NodeKind::Function, "load_user"),
            ],
            edges: vec![GraphEdge {
                source: NodeRef(0),
                target: NodeRef(1),
                kind: EdgeKind::Calls,
                call_args: Some(vec!["OrderId".into()]),
                type_arg_bindings: Some(vec![TypeArgBinding {
                    param: "T".into(),
                    ty: "UserId".into(),
                }]),
                effect_arg_bindings: None,
                capability_arg_bindings: None,
            }],
        };

        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let e = &decoded.edges[0];
        assert_eq!(
            e.call_args.as_deref(),
            Some(["OrderId".to_string()].as_ref())
        );
        assert_eq!(e.type_arg_bindings.as_ref().unwrap()[0].ty, "UserId");
    }

    // ── G24: TRIANGULATE – node_without_new_fields_unchanged ─────────────
    // Backward compat: existing nodes without new fields round-trip unchanged.
    //   GIVEN a node created with GraphNode::new (all new fields None)
    //   WHEN serialized to CBOR and deserialized
    //   THEN all new optional fields remain None (not None → default junk)
    #[test]
    fn node_without_new_fields_unchanged() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![node(0, NodeKind::Function, "legacy_fn")],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        let n = &decoded.nodes[0];
        assert!(
            n.generic_params.is_none(),
            "generic_params must be None for legacy node"
        );
        assert!(n.params.is_none(), "params must be None for legacy node");
        assert!(
            n.return_type.is_none(),
            "return_type must be None for legacy node"
        );
        assert!(
            n.interface_impls.is_none(),
            "interface_impls must be None for legacy node"
        );
        assert!(
            n.refinement_ref.is_none(),
            "refinement_ref must be None for legacy node"
        );
        assert!(
            n.constraint_set.is_none(),
            "constraint_set must be None for legacy node"
        );
    }

    // ── TRIANGULATE: different_graphs_produce_different_bytes ────────────
    // Forces non-trivial encoding: two distinct graphs must NOT hash the same.
    #[test]
    fn different_graphs_produce_different_bytes() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let graph_a = SemanticGraph {
            nodes: vec![node(0, NodeKind::Module, "a")],
            edges: vec![],
        };
        let graph_b = SemanticGraph {
            nodes: vec![node(0, NodeKind::Module, "b")], // different name
            edges: vec![],
        };

        let bytes_a = codec.encode(&graph_a).expect("encode a");
        let bytes_b = codec.encode(&graph_b).expect("encode b");
        assert_ne!(
            bytes_a, bytes_b,
            "graphs with different content must produce different CBOR bytes"
        );
    }

    // ── G32: Semantic ref newtypes ────────────────────────────────────────

    // Spec: Each newtype wraps a String and is constructible.
    // RED: tests written before types existed; now GREEN after add.
    #[test]
    fn ref_newtypes_are_constructible() {
        let block = BlockRef("block_checkout".to_string());
        let contract = ContractRef("contract.payment".to_string());
        let effect = EffectRef("effect.db.read".to_string());
        let proof = ProofObligationRef("proof.invariant.balance".to_string());
        let rtcheck = RuntimeCheckRef("rtcheck.null_guard".to_string());

        assert_eq!(block.0, "block_checkout");
        assert_eq!(contract.0, "contract.payment");
        assert_eq!(effect.0, "effect.db.read");
        assert_eq!(proof.0, "proof.invariant.balance");
        assert_eq!(rtcheck.0, "rtcheck.null_guard");
    }

    // TRIANGULATE: two different values of the same newtype are not equal.
    #[test]
    fn ref_newtypes_inequality() {
        let a = BlockRef("block_a".to_string());
        let b = BlockRef("block_b".to_string());
        assert_ne!(
            a, b,
            "BlockRef with different inner values must not be equal"
        );

        let ca = ContractRef("c1".to_string());
        let cb = ContractRef("c2".to_string());
        assert_ne!(ca, cb);
    }

    // Spec: Ref newtypes are serde-transparent — CBOR encoding matches plain String.
    #[test]
    fn ref_newtype_cbor_is_transparent_with_string() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let raw = "block_checkout_flow".to_string();
        let typed = BlockRef(raw.clone());

        let bytes_raw = codec.encode(&raw).expect("encode raw string");
        let bytes_typed = codec.encode(&typed).expect("encode BlockRef");

        assert_eq!(
            bytes_raw, bytes_typed,
            "BlockRef CBOR must be identical to plain String CBOR (transparent serde)"
        );
    }

    // TRIANGULATE: Ref newtype CBOR round-trip preserves value.
    #[test]
    fn ref_newtype_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let original = ContractRef("contract.checkout.payment".to_string());
        let bytes = codec.encode(&original).expect("encode ContractRef");
        let decoded: ContractRef = codec.decode(&bytes).expect("decode ContractRef");
        assert_eq!(
            original, decoded,
            "ContractRef must survive CBOR round-trip"
        );
    }

    // ── Task C3 (RED): ConstraintSet::has_partial_ord ────────────────────

    // S-C3a: ConstraintSet with has_partial_ord=true round-trips through CBOR.
    #[test]
    fn constraint_set_with_has_partial_ord_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "Price");
        node.constraint_set = Some(ConstraintSet {
            has_eq: true,
            has_ord: false,
            has_hash: false,
            has_partial_ord: true,
            extras: vec![],
        });
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        let cs = decoded.nodes[0]
            .constraint_set
            .as_ref()
            .expect("constraint_set must be Some");
        assert!(
            cs.has_partial_ord,
            "has_partial_ord must be true after round-trip"
        );
        assert!(!cs.has_ord, "has_ord must remain false");
    }

    // S-C3b: Old ConstraintSet without has_partial_ord deserializes with has_partial_ord=false.
    // Backward compatibility via serde default.
    #[test]
    fn legacy_constraint_set_has_partial_ord_defaults_false() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        // A legacy node with constraint_set that has no has_partial_ord field
        // in its CBOR bytes must deserialize with has_partial_ord=false.
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "Amount");
        node.constraint_set = Some(ConstraintSet {
            has_eq: true,
            has_ord: true,
            has_hash: false,
            has_partial_ord: false, // default — must not be emitted in CBOR when false
            extras: vec![],
        });
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        let cs = decoded.nodes[0].constraint_set.as_ref().unwrap();
        assert!(!cs.has_partial_ord, "has_partial_ord must default to false");
        assert!(cs.has_ord, "has_ord must be preserved");
    }

    // ── Task D1 (RED): new NodeKind variants ──────────────────────────────

    // S-D1a: NodeKind::Interface, Impl, EffectAlias are constructible.
    #[test]
    fn new_node_kind_variants_are_constructible() {
        let _interface = NodeKind::Interface;
        let _impl_kind = NodeKind::Impl;
        let _effect_alias = NodeKind::EffectAlias;
        // All constructed without panic — test passes.
    }

    // S-D1b: Interface node CBOR round-trip preserves kind.
    #[test]
    fn interface_node_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![GraphNode::new(
                NodeRef(0),
                NodeKind::Interface,
                "PaymentProvider",
            )],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(
            decoded.nodes[0].kind,
            NodeKind::Interface,
            "Interface kind must be preserved through CBOR round-trip"
        );
    }

    // S-D1c: Impl node round-trips and passes validation.
    // Triangulation: Impl is distinct from Interface in CBOR encoding.
    #[test]
    fn impl_node_round_trips_and_passes_validation() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Interface, "Chargeable"),
                GraphNode::new(NodeRef(1), NodeKind::Impl, "StripeChargeImpl"),
            ],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded.nodes[0].kind, NodeKind::Interface);
        assert_eq!(decoded.nodes[1].kind, NodeKind::Impl);
        assert_eq!(
            decoded.validate(),
            Ok(()),
            "graph with Impl node must validate"
        );
    }

    // S-D1d: EffectAlias node round-trips.
    #[test]
    fn effect_alias_node_round_trips() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![GraphNode::new(
                NodeRef(0),
                NodeKind::EffectAlias,
                "DatabaseAlias",
            )],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded.nodes[0].kind, NodeKind::EffectAlias);
    }

    // ── Task D3 (RED): HandlerMeta on GraphNode ───────────────────────────

    // S-D3a: HandlerMeta with handled_caps is constructible and round-trips.
    #[test]
    fn handler_meta_with_caps_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "stripe_handler");
        node.handler_meta = Some(HandlerMeta {
            handled_caps: vec!["database.read".to_string(), "payments.charge".to_string()],
            internal_effects: vec!["IO".to_string()],
            satisfies_contract: Some("cap.payments.Chargeable".to_string()),
        });
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        let hm = decoded.nodes[0]
            .handler_meta
            .as_ref()
            .expect("handler_meta must be Some");
        assert_eq!(hm.handled_caps, ["database.read", "payments.charge"]);
        assert_eq!(hm.internal_effects, ["IO"]);
        assert_eq!(
            hm.satisfies_contract.as_deref(),
            Some("cap.payments.Chargeable")
        );
    }

    // S-D3b: Old GraphNode without handler_meta deserializes with handler_meta=None.
    // Backward compatibility: existing fixtures must not break.
    #[test]
    fn legacy_node_without_handler_meta_has_none() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "legacy_fn")],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert!(
            decoded.nodes[0].handler_meta.is_none(),
            "legacy node must have handler_meta=None after CBOR round-trip"
        );
    }

    // ── Task C1 (RED): EffectArgBinding and CapabilityArgBinding on GraphEdge ──
    // Tests written BEFORE the structs and fields exist — compilation fails = RED.

    // C1-1: EffectArgBinding is constructible and fields are correct.
    // Spec scenario: "EffectArgBinding CBOR round-trip"
    #[test]
    fn effect_arg_binding_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let binding = EffectArgBinding {
            param: "e".to_string(),
            effects: vec!["IO".to_string()],
        };
        let bytes = codec.encode(&binding).expect("encode EffectArgBinding");
        let decoded: EffectArgBinding = codec.decode(&bytes).expect("decode EffectArgBinding");
        assert_eq!(decoded.param, "e");
        assert_eq!(decoded.effects, ["IO"]);
        assert_eq!(decoded, binding);
    }

    // C1-2: GraphEdge with effect_arg_bindings round-trips through CBOR.
    // Spec scenario: "EffectArgBinding CBOR round-trip" (on an edge)
    #[test]
    fn graph_edge_with_effect_arg_bindings_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Function, "caller"),
                node(1, NodeKind::Function, "callee"),
            ],
            edges: vec![GraphEdge {
                source: NodeRef(0),
                target: NodeRef(1),
                kind: EdgeKind::Calls,
                call_args: None,
                type_arg_bindings: None,
                effect_arg_bindings: Some(vec![EffectArgBinding {
                    param: "e".to_string(),
                    effects: vec!["IO".to_string()],
                }]),
                capability_arg_bindings: None,
            }],
        };

        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let bindings = decoded.edges[0]
            .effect_arg_bindings
            .as_ref()
            .expect("effect_arg_bindings must be Some");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].param, "e");
        assert_eq!(bindings[0].effects, ["IO"]);
    }

    // C1-3: Edge without effect_arg_bindings is backward compatible (None after decode).
    // Spec scenario: "Edge without effect_arg_bindings is backward compatible"
    #[test]
    fn edge_without_effect_arg_bindings_is_backward_compat() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        // Simulate an edge encoded before EffectArgBinding field existed.
        // Creating it with the new constructor (None fields) produces identical
        // bytes to the old format (serde skips None).
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Function, "f"),
                node(1, NodeKind::Function, "g"),
            ],
            edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls)],
        };

        let bytes = codec.encode(&graph).expect("encode legacy edge");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert!(
            decoded.edges[0].effect_arg_bindings.is_none(),
            "legacy edge must decode with effect_arg_bindings=None"
        );
        assert!(
            decoded.edges[0].capability_arg_bindings.is_none(),
            "legacy edge must decode with capability_arg_bindings=None"
        );
    }

    // C1-4: CapabilityArgBinding is constructible and round-trips through CBOR.
    // Spec scenario: "CapabilityArgBinding" struct
    #[test]
    fn capability_arg_binding_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let binding = CapabilityArgBinding {
            param: "cap".to_string(),
            caps: vec!["net:read".to_string()],
        };
        let bytes = codec.encode(&binding).expect("encode CapabilityArgBinding");
        let decoded: CapabilityArgBinding =
            codec.decode(&bytes).expect("decode CapabilityArgBinding");
        assert_eq!(decoded.param, "cap");
        assert_eq!(decoded.caps, ["net:read"]);
        assert_eq!(decoded, binding);
    }

    // C1-5 (TRIANGULATE): GraphEdge with both new fields round-trips.
    // Forces the real implementation to handle both fields simultaneously.
    #[test]
    fn graph_edge_with_both_arg_binding_fields_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Function, "caller"),
                node(1, NodeKind::Function, "callee"),
            ],
            edges: vec![GraphEdge {
                source: NodeRef(0),
                target: NodeRef(1),
                kind: EdgeKind::Calls,
                call_args: None,
                type_arg_bindings: None,
                effect_arg_bindings: Some(vec![EffectArgBinding {
                    param: "e".to_string(),
                    effects: vec!["IO".to_string()],
                }]),
                capability_arg_bindings: Some(vec![CapabilityArgBinding {
                    param: "cap".to_string(),
                    caps: vec!["net:read".to_string()],
                }]),
            }],
        };

        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        assert!(decoded.edges[0].effect_arg_bindings.is_some());
        assert!(decoded.edges[0].capability_arg_bindings.is_some());
    }

    // S-D3c: HandlerMeta without satisfies_contract omits that field.
    // Triangulation: None satisfies_contract must not appear in CBOR.
    #[test]
    fn handler_meta_without_contract_omits_field() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let mut node_with = GraphNode::new(NodeRef(0), NodeKind::Function, "h");
        node_with.handler_meta = Some(HandlerMeta {
            handled_caps: vec!["db.read".to_string()],
            internal_effects: vec![],
            satisfies_contract: Some("SomeContract".to_string()),
        });
        let mut node_without = GraphNode::new(NodeRef(0), NodeKind::Function, "h");
        node_without.handler_meta = Some(HandlerMeta {
            handled_caps: vec!["db.read".to_string()],
            internal_effects: vec![],
            satisfies_contract: None,
        });
        let bytes_with = codec
            .encode(&SemanticGraph {
                nodes: vec![node_with],
                edges: vec![],
            })
            .expect("encode with");
        let bytes_without = codec
            .encode(&SemanticGraph {
                nodes: vec![node_without],
                edges: vec![],
            })
            .expect("encode without");
        // Node with satisfies_contract must encode to MORE bytes.
        assert!(
            bytes_with.len() > bytes_without.len(),
            "satisfies_contract=Some must produce more bytes than None"
        );
    }

    // ── validate_full: valid graph returns empty errors ───────────────────
    // Spec: validate_full on a clean graph returns zero errors.
    //
    // RED: validate_full() did not exist → compile error.
    // GREEN: method added with all checks → returns empty vec.
    #[test]
    fn validate_full_valid_graph_returns_no_errors() {
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "core"),
                node(1, NodeKind::Function, "run"),
                node(2, NodeKind::Effect, "io"),
            ],
            edges: vec![edge(0, 1, EdgeKind::DependsOn), edge(1, 2, EdgeKind::Emits)],
        };
        let errors = graph.validate_full();
        assert!(
            errors.is_empty(),
            "clean graph must produce zero errors; got: {errors:?}"
        );
    }

    // ── validate_full: duplicate ref detected ────────────────────────────
    // Spec: validate_full returns DuplicateRef for duplicate NodeRef(0).
    #[test]
    fn validate_full_detects_duplicate_ref() {
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "a"),
                node(0, NodeKind::Function, "b"), // duplicate
            ],
            edges: vec![],
        };
        let errors = graph.validate_full();
        assert!(
            errors.contains(&GraphValidationError::DuplicateRef(NodeRef(0))),
            "must detect duplicate NodeRef(0); got: {errors:?}"
        );
    }

    // ── validate_full: dangling edge detected ─────────────────────────────
    // TRIANGULATE: different error kind from duplicate.
    // Spec: validate_full returns DanglingEdge for missing edge endpoint.
    #[test]
    fn validate_full_detects_dangling_edge() {
        let graph = SemanticGraph {
            nodes: vec![node(0, NodeKind::Module, "src")],
            edges: vec![edge(0, 99, EdgeKind::DependsOn)], // target 99 missing
        };
        let errors = graph.validate_full();
        assert!(
            errors.contains(&GraphValidationError::DanglingEdge {
                r#ref: NodeRef(99),
                role: DanglingRole::Target,
            }),
            "must detect dangling target NodeRef(99); got: {errors:?}"
        );
    }

    // ── validate_full: effect_row without Emits edge is rejected ─────────
    // Spec: A node with non-empty effect_row but no Emits edge is incoherent.
    //
    // RED: EffectRowNoEmitsEdge variant did not exist → compile error.
    // GREEN: Pass 3 in validate_full() detects the missing edge.
    #[test]
    fn validate_full_detects_effect_row_without_emits_edge() {
        let mut fn_node = node(0, NodeKind::Function, "pay");
        fn_node.effect_row = Some(EffectRow {
            effects: vec!["IO".to_string()],
        });
        let graph = SemanticGraph {
            nodes: vec![fn_node],
            edges: vec![], // no Emits edge!
        };
        let errors = graph.validate_full();
        assert!(
            errors.contains(&GraphValidationError::EffectRowNoEmitsEdge(NodeRef(0))),
            "must detect effect_row without Emits edge; got: {errors:?}"
        );
    }

    // ── validate_full: effect_row WITH Emits edge passes ─────────────────
    // TRIANGULATE: coherent effect_row must not produce an error.
    #[test]
    fn validate_full_effect_row_with_emits_edge_passes() {
        let mut fn_node = node(0, NodeKind::Function, "pay");
        fn_node.effect_row = Some(EffectRow {
            effects: vec!["IO".to_string()],
        });
        let io_node = node(1, NodeKind::Effect, "io");
        let graph = SemanticGraph {
            nodes: vec![fn_node, io_node],
            edges: vec![edge(0, 1, EdgeKind::Emits)],
        };
        let errors = graph.validate_full();
        let effect_row_errors: Vec<_> = errors
            .iter()
            .filter(|e| matches!(e, GraphValidationError::EffectRowNoEmitsEdge(_)))
            .collect();
        assert!(
            effect_row_errors.is_empty(),
            "coherent effect_row+Emits must not produce EffectRowNoEmitsEdge; got: {errors:?}"
        );
    }

    // ── validate_full: capability_reqs missing Capability node ───────────
    // Spec: A capability requirement that names a non-existent Capability node
    // is incoherent.
    //
    // RED: CapabilityReqsMissingNode variant did not exist → compile error.
    // GREEN: Pass 4 in validate_full() detects the missing node.
    #[test]
    fn validate_full_detects_capability_req_missing_node() {
        let mut fn_node = node(0, NodeKind::Function, "transfer");
        fn_node.capability_reqs = Some(CapabilityReqs {
            caps: vec!["net:read".to_string()],
        });
        let graph = SemanticGraph {
            nodes: vec![fn_node],
            edges: vec![], // no Capability node named "net:read"
        };
        let errors = graph.validate_full();
        assert!(
            errors.contains(&GraphValidationError::CapabilityReqsMissingNode {
                owner_ref: NodeRef(0),
                cap_name: "net:read".to_string(),
            }),
            "must detect missing Capability node 'net:read'; got: {errors:?}"
        );
    }

    // ── validate_full: capability_reqs WITH matching Capability node passes
    // TRIANGULATE: satisfied capability_reqs must not produce an error.
    #[test]
    fn validate_full_capability_reqs_with_matching_node_passes() {
        let mut fn_node = node(0, NodeKind::Function, "transfer");
        fn_node.capability_reqs = Some(CapabilityReqs {
            caps: vec!["net:read".to_string()],
        });
        let cap_node = node(1, NodeKind::Capability, "net:read");
        let graph = SemanticGraph {
            nodes: vec![fn_node, cap_node],
            edges: vec![],
        };
        let errors = graph.validate_full();
        let cap_errors: Vec<_> = errors
            .iter()
            .filter(|e| matches!(e, GraphValidationError::CapabilityReqsMissingNode { .. }))
            .collect();
        assert!(
            cap_errors.is_empty(),
            "satisfied capability_reqs must not produce errors; got: {errors:?}"
        );
    }

    // ── validate_full: multiple errors returned at once ───────────────────
    // Spec: validate_full returns ALL errors, not just the first one.
    #[test]
    fn validate_full_returns_all_errors() {
        // Two duplicate refs AND a dangling edge
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "a"),
                node(0, NodeKind::Function, "b"), // duplicate NodeRef(0)
            ],
            edges: vec![edge(0, 99, EdgeKind::DependsOn)], // dangling target 99
        };
        let errors = graph.validate_full();
        // Must contain at least DuplicateRef and DanglingEdge
        let has_dup = errors
            .iter()
            .any(|e| matches!(e, GraphValidationError::DuplicateRef(NodeRef(0))));
        let has_dangling = errors.iter().any(|e| {
            matches!(
                e,
                GraphValidationError::DanglingEdge {
                    r#ref: NodeRef(99),
                    role: DanglingRole::Target,
                }
            )
        });
        assert!(
            has_dup,
            "validate_full must include DuplicateRef error; got: {errors:?}"
        );
        assert!(
            has_dangling,
            "validate_full must include DanglingEdge error; got: {errors:?}"
        );
        assert!(
            errors.len() >= 2,
            "validate_full must return all errors, not just one; got: {errors:?}"
        );
    }
}
