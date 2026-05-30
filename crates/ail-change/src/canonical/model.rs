use super::*;

pub struct CanonicalMeta {
    /// Identity of the change author.
    pub author: String,
    /// Human-readable description; always non-empty after canonicalization.
    pub description: String,
    /// When the changeset was created.
    pub timestamp: Timestamp,
}

// ── OpPayload ─────────────────────────────────────────────────────────────

/// Concrete graph mutation payload for a `CanonicalOp`.
///
/// For ops originating from a raw `ChangeSet` (which has no payload data),
/// `canonicalize` produces `Noop`. Apply tests construct `CanonicalOp`s
/// directly with real payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpPayload {
    /// Create a new node in the graph.
    CreateNode(Box<GraphNode>),
    /// Add a directed edge to the graph.
    AddEdge(GraphEdge),
    /// Remove an existing node by ref.
    RemoveNode(NodeRef),
    /// Rename a node (minimal Set semantics).
    SetNodeName { node_id: NodeRef, name: String },
    /// Remove an existing node by stable graph name.
    RemoveNodeByName(String),
    /// Rename a node by stable graph name.
    RenameNodeByName { target: String, name: String },
    /// Add a directed edge after resolving endpoint names during apply.
    AddEdgeByName {
        source: String,
        target: String,
        kind: EdgeKind,
    },
    /// Remove a directed edge after resolving endpoint names during apply.
    RemoveEdgeByName {
        source: String,
        target: String,
        kind: EdgeKind,
    },
    /// Set a function return type by stable graph name.
    SetReturnByName { target: String, ty: String },
    /// Set a function expression body by stable graph name.
    SetBodyByName { target: String, body: String },
    /// Set a supported scalar metadata field by stable graph name.
    SetMetadataByName {
        target: String,
        key: String,
        value: String,
    },
    /// Add a function parameter by stable graph name.
    AddParamByName {
        target: String,
        name: String,
        ty: String,
    },
    /// Add an effect to a node's effect row by stable graph name.
    AddEffectByName { target: String, effect: String },
    /// Remove an effect from a node's effect row by stable graph name.
    RemoveEffectByName { target: String, effect: String },
    /// Add a contract clause to a node by stable graph name.
    AddContractByName {
        target: String,
        kind: String,
        rule: String,
    },
    /// Remove a contract clause from a node by stable graph name.
    RemoveContractByName { target: String, rule: String },
    /// Add a capability requirement to a node by stable graph name.
    AddCapabilityReqByName { target: String, capability: String },
    /// Remove a capability requirement from a node by stable graph name.
    RemoveCapabilityReqByName { target: String, capability: String },
    /// Set export visibility by stable graph name.
    SetVisibilityByName {
        target: String,
        visibility: Visibility,
    },
    /// Add a binding to a node by stable graph name.
    AddBindingByName { target: String, binding: Binding },
    /// Add an inferred fact to a node by stable graph name.
    AddInferredFactByName { target: String, fact: InferredFact },
    /// Add a derived implementation to a node by stable graph name.
    AddDerivedImplByName { target: String, impl_name: String },
    /// Add a generated artifact reference to a node by stable graph name.
    AddGeneratedArtifactByName {
        target: String,
        artifact: GeneratedArtifact,
    },
    /// Add a compile-time assertion to a node by stable graph name.
    AddAssertionByName {
        target: String,
        assertion: Assertion,
    },
    /// Set workflow state by stable graph name.
    SetWorkflowStateByName {
        target: String,
        state: WorkflowState,
    },
    /// No-op placeholder; used for raw ChangeSet ops or malformed parsed ops.
    Noop,
}

// ── CanonicalOp ───────────────────────────────────────────────────────────

/// A single canonicalized operation: phase classifier + payload + block hash.
///
/// `verb` holds the full verb string (e.g. `"create_function"`).
/// `args` holds the kv arguments with all applicable defaults materialized.
/// For ops originating from the legacy `canonicalize(ChangeSet)` path,
/// `verb` is the empty string and `args` is empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalOp {
    /// Phase classifier (used for ordering and labelling).
    pub kind: ChangeSetOp,
    /// Full verb as written in the source (e.g. `"create_function"`).
    /// Empty string for ops produced by the legacy `canonicalize` path.
    pub verb: String,
    /// Key/value arguments with defaults materialized.
    /// Empty for ops produced by the legacy `canonicalize` path.
    #[serde(default)]
    pub args: OpArgs,
    /// Concrete graph mutation payload.
    pub payload: OpPayload,
    /// blake3 hash of this op's canonical encoding.
    pub block_hash: BlockHash,
}

impl Default for CanonicalOp {
    fn default() -> Self {
        Self {
            kind: ChangeSetOp::Infer,
            verb: String::new(),
            args: BTreeMap::new(),
            payload: OpPayload::Noop,
            block_hash: BlockHash([0u8; 32]),
        }
    }
}

// ── Precondition ──────────────────────────────────────────────────────────

/// A precondition evaluated before ops are applied.
///
/// If any precondition fails, `apply` returns `Failed` and restores the
/// pre-apply graph clone (rollback).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Precondition {
    /// The referenced node must exist in the graph (numeric NodeRef form).
    AssertExists(AssertExists),
    /// The referenced node's canonical hash must match the expected value (numeric form).
    AssertHash(AssertHash),
    /// The named node (e.g. `type.Cart`) must exist in the graph.
    AssertExistsByName(String),
    /// The named node's canonical hash must match the expected value.
    AssertHashByName {
        /// Stable graph name of the node (e.g. `fn.cart_total`).
        name: String,
        /// Expected blake3 hash of the node's canonical encoding.
        expected_hash: BlockHash,
    },
    /// A context slice hash assertion: verifies that the named node exists
    /// and — when a context hash is supplied — that it matches the recorded
    /// context slice hash (tool: `ail context <target>`).
    AssertContext {
        /// Stable graph name of the target (e.g. `fn.checkout`).
        target_name: String,
        /// Optional context hash returned by `ail context <target> --json`.
        context_hash: Option<String>,
    },
}

// ── CanonicalChangeSet ────────────────────────────────────────────────────

/// A fully canonicalized changeset ready for atomic application.
///
/// Constructed either via `canonicalize(ChangeSet)` / `canonicalize_parsed`
/// or directly in tests when explicit payloads are required.
///
/// All optional sections from the submitted form (`expect`, `approval`,
/// `composition`, `blocks`, `verify`) are carried through so downstream
/// consumers (verifier, policy engine) can inspect them without re-parsing
/// the submitted text.
///
/// ## Schema versions (§Versioning y schema evolution)
///
/// The five schema version fields are carried through from `ParsedChangeSet`
/// unchanged. They default to `None` when absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalChangeSet {
    /// Canonicalized authorship and intent metadata.
    pub meta: CanonicalMeta,
    /// Snapshot identity against which this changeset was authored.
    pub base_snapshot_id: SnapshotId,
    /// ACL language syntax version (defaults to `"1.0"`).
    #[serde(default = "default_acl_version")]
    pub acl_version: String,
    /// Op-schema version declared by this changeset (`op_schema <N>`).
    #[serde(default)]
    pub op_schema_version: Option<String>,
    /// Semantic Graph schema version (`graph_schema <N>`).
    #[serde(default)]
    pub graph_schema_version: Option<String>,
    /// Core IR schema version (`core_ir_schema <N>`).
    #[serde(default)]
    pub core_ir_schema_version: Option<String>,
    /// Diagnostics format version (`diagnostics_schema <N>`).
    #[serde(default)]
    pub diagnostics_schema_version: Option<String>,
    /// Verification report format version (`verification_schema <N>`).
    #[serde(default)]
    pub verification_schema_version: Option<String>,
    /// Preconditions evaluated before any op is applied.
    pub preconditions: Vec<Precondition>,
    /// Phase-ordered, hash-stamped operations.
    pub ops: Vec<CanonicalOp>,
    /// AI-written claims about the expected diff (carried for verifier).
    #[serde(default)]
    pub expect: Option<ExpectClaims>,
    /// Approval requirements declared in the submitted form.
    #[serde(default)]
    pub approval: Option<ApprovalRequirements>,
    /// Cross-changeset composition relationships.
    #[serde(default)]
    pub composition: ChangeComposition,
    /// Typed block sections (expressions, schemas, docs, etc.).
    #[serde(default)]
    pub blocks: Vec<ParsedBlock>,
    /// Verify directives (short form and block form lines combined).
    #[serde(default)]
    pub verify: Vec<String>,
}

impl Default for CanonicalChangeSet {
    fn default() -> Self {
        Self {
            meta: CanonicalMeta {
                author: String::new(),
                description: String::new(),
                timestamp: Timestamp(0),
            },
            base_snapshot_id: SnapshotId(0),
            acl_version: default_acl_version(),
            op_schema_version: None,
            graph_schema_version: None,
            core_ir_schema_version: None,
            diagnostics_schema_version: None,
            verification_schema_version: None,
            preconditions: Vec::new(),
            ops: Vec::new(),
            expect: None,
            approval: None,
            composition: ChangeComposition::default(),
            blocks: Vec::new(),
            verify: Vec::new(),
        }
    }
}

fn default_acl_version() -> String {
    "1.0".to_string()
}
