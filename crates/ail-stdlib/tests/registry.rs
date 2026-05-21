// Integration tests for ail-stdlib PR 1 slice.
//
// Covers spec scenarios: StabilityTier, CBOR round-trips, deterministic hash,
// graph projection infrastructure, and capability constant contracts.
//
// Tests that require v1_registry() (4.6–4.10) are deferred to PR 2.

use ail_core::semantic_graph::{
    CapabilityReqs, ContractClauses, EffectRow, NodeKind, NodeRef, SemanticGraph, TypeFacts,
};
use ail_stdlib::{
    capability,
    registry::{StabilityTier, StdlibEntry, StdlibId, StdlibRegistry},
};

// ── helpers ───────────────────────────────────────────────────────────────

/// Minimal entry: only required fields; all optionals are `None`.
fn minimal_entry() -> StdlibEntry {
    StdlibEntry {
        id: StdlibId("std.core".to_string()),
        module_path: "std::core".to_string(),
        name: "core".to_string(),
        kind: NodeKind::Module,
        stability: StabilityTier::Stable,
        type_facts: None,
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
    }
}

// ── Spec: StabilityTier ───────────────────────────────────────────────────
//
// Requirement: StabilityTier
// "MUST define a StabilityTier enum with exactly five variants … comparable for equality."

// Spec scenario: All five variants are representable
//   GIVEN a StabilityTier value constructed from any of the five named variants
//   WHEN compared for equality with itself
//   THEN the result is true
//
// RED: StabilityTier doesn't exist yet.
#[test]
fn stability_tier_all_five_variants_representable() {
    assert_eq!(StabilityTier::Stable, StabilityTier::Stable);
    assert_eq!(StabilityTier::Experimental, StabilityTier::Experimental);
    assert_eq!(StabilityTier::Deprecated, StabilityTier::Deprecated);
    assert_eq!(StabilityTier::Unsafe, StabilityTier::Unsafe);
    assert_eq!(StabilityTier::Internal, StabilityTier::Internal);
}

// Spec scenario: Variants are distinct
//   GIVEN StabilityTier::Stable and StabilityTier::Deprecated
//   WHEN compared
//   THEN they are not equal
#[test]
fn stability_tier_variants_are_distinct() {
    assert_ne!(StabilityTier::Stable, StabilityTier::Deprecated);
    // Triangulate: other pairs are also distinct
    assert_ne!(StabilityTier::Experimental, StabilityTier::Internal);
    assert_ne!(StabilityTier::Unsafe, StabilityTier::Stable);
}

// ── Spec: StdlibEntry CBOR round-trips ───────────────────────────────────
//
// Requirement: StdlibEntry fields
// "required fields … and optional fields … (all Option<_>, matching ail-core types)"

// Spec scenario: Minimal entry CBOR round-trip
//   GIVEN a StdlibEntry constructed with required fields only (all optionals None)
//   WHEN serialized to CBOR then deserialized
//   THEN the result equals the original entry
#[test]
fn minimal_entry_cbor_round_trip() {
    let entry = minimal_entry();
    let reg = StdlibRegistry {
        entries: vec![entry.clone()],
    };
    let bytes = reg.cbor_bytes().expect("cbor_bytes must succeed");
    let decoded = StdlibRegistry::from_cbor_bytes(&bytes).expect("from_cbor_bytes must succeed");
    assert_eq!(decoded.entries.len(), 1);
    assert_eq!(decoded.entries[0], entry);
}

// Spec scenario: Full entry CBOR round-trip
//   GIVEN a StdlibEntry with all optional fields populated
//   WHEN serialized to CBOR then deserialized
//   THEN the result equals the original entry
#[test]
fn full_entry_cbor_round_trip() {
    let entry = StdlibEntry {
        id: StdlibId("std.core.Bool".to_string()),
        module_path: "std::core".to_string(),
        name: "Bool".to_string(),
        kind: NodeKind::Type,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec![],
        }),
        effect_row: Some(EffectRow {
            effects: vec!["Pure".to_string()],
        }),
        capability_reqs: Some(CapabilityReqs {
            caps: vec![capability::CLOCK_NOW.to_string()],
        }),
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is bool".to_string()],
            ensures: vec!["result is bool".to_string()],
        }),
    };
    let reg = StdlibRegistry {
        entries: vec![entry.clone()],
    };
    let bytes = reg.cbor_bytes().expect("cbor_bytes must succeed");
    let decoded = StdlibRegistry::from_cbor_bytes(&bytes).expect("from_cbor_bytes must succeed");
    assert_eq!(decoded.entries[0], entry);
}

// ── Spec: Deterministic registry hash ────────────────────────────────────
//
// Requirement: Deterministic registry hash
// "hash() MUST return [u8; 32] computed as BLAKE3 of the CBOR encoding …"

// Spec scenario: Hash is stable across two calls
//   GIVEN a populated StdlibRegistry
//   WHEN hash() is called twice on the same instance
//   THEN both returned [u8; 32] values are identical
#[test]
fn hash_stable_across_two_calls() {
    let reg = StdlibRegistry {
        entries: vec![minimal_entry()],
    };
    let h1 = reg.hash().expect("first hash must succeed");
    let h2 = reg.hash().expect("second hash must succeed");
    assert_eq!(h1, h2, "hash() must be deterministic");
}

// Spec scenario: Distinct registries produce distinct hashes
//   GIVEN two StdlibRegistry instances differing by at least one entry
//   WHEN hash() is called on each
//   THEN the two [u8; 32] results are not equal
#[test]
fn distinct_registries_produce_distinct_hashes() {
    let reg_a = StdlibRegistry {
        entries: vec![minimal_entry()],
    };
    let reg_b = StdlibRegistry {
        entries: vec![StdlibEntry {
            id: StdlibId("std.option".to_string()),
            module_path: "std::option".to_string(),
            name: "option".to_string(),
            ..minimal_entry()
        }],
    };
    let h_a = reg_a.hash().expect("hash a");
    let h_b = reg_b.hash().expect("hash b");
    assert_ne!(h_a, h_b, "distinct registries must have distinct hashes");
}

// ── Spec: Graph projection ────────────────────────────────────────────────
//
// Requirement: Graph projection
// "to_graph_nodes() MUST return Vec<GraphNode> …
//  Inserting all returned nodes into an empty SemanticGraph MUST pass validate()"
//
// Note: the v1_registry-based tests (4.6–4.8) are deferred to PR 2.
// The following tests cover the projection infrastructure with a hand-built registry.

// Triangulation for task 2.5 — sequential NodeRef assignment
#[test]
fn to_graph_nodes_assigns_sequential_node_refs() {
    let reg = StdlibRegistry {
        entries: vec![
            StdlibEntry {
                id: StdlibId("std.core".to_string()),
                module_path: "std::core".to_string(),
                name: "core".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            StdlibEntry {
                id: StdlibId("std.option".to_string()),
                module_path: "std::option".to_string(),
                name: "option".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
        ],
    };
    let nodes = reg.to_graph_nodes();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].id, NodeRef(0));
    assert_eq!(nodes[1].id, NodeRef(1));
}

// Triangulation — graph node name is the StdlibId string (design contract)
#[test]
fn to_graph_nodes_uses_stdlib_id_as_name() {
    let reg = StdlibRegistry {
        entries: vec![minimal_entry()],
    };
    let nodes = reg.to_graph_nodes();
    assert_eq!(nodes[0].name, "std.core");
}

// Triangulation — NodeKind is preserved from StdlibEntry to GraphNode
#[test]
fn to_graph_nodes_preserves_node_kind() {
    let reg = StdlibRegistry {
        entries: vec![StdlibEntry {
            id: StdlibId("std.capability".to_string()),
            module_path: "std::capability".to_string(),
            name: "capability".to_string(),
            kind: NodeKind::Capability,
            stability: StabilityTier::Stable,
            type_facts: None,
            effect_row: None,
            capability_reqs: None,
            contract_clauses: None,
        }],
    };
    let nodes = reg.to_graph_nodes();
    assert_eq!(nodes[0].kind, NodeKind::Capability);
}

// Projection: inserting projected nodes into an empty SemanticGraph passes validate()
#[test]
fn projected_nodes_pass_semantic_graph_validation() {
    let reg = StdlibRegistry {
        entries: vec![
            StdlibEntry {
                id: StdlibId("std.core".to_string()),
                module_path: "std::core".to_string(),
                name: "core".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            StdlibEntry {
                id: StdlibId("std.option".to_string()),
                module_path: "std::option".to_string(),
                name: "option".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
        ],
    };
    let nodes = reg.to_graph_nodes();
    let graph = SemanticGraph {
        nodes,
        edges: vec![],
    };
    assert_eq!(
        graph.validate(),
        Ok(()),
        "projected nodes must pass SemanticGraph::validate()"
    );
}

// ── Spec: Capability name constants ──────────────────────────────────────
//
// Requirement: Capability name constants
// "All capability strings … MUST be declared as pub const &str constants"

// Spec scenario: Constant round-trips through CapabilityReqs
//   GIVEN a capability constant from ail-stdlib (e.g. "clock.now")
//   WHEN inserted into CapabilityReqs::caps and retrieved
//   THEN the result equals the original constant
#[test]
fn capability_constant_round_trips_through_capability_reqs() {
    let reqs = CapabilityReqs {
        caps: vec![capability::CLOCK_NOW.to_string()],
    };
    assert_eq!(reqs.caps[0], capability::CLOCK_NOW);
}

// Triangulate: all constants are lower-dotted strings (no uppercase, no spaces)
#[test]
fn capability_constants_are_lower_dotted_strings() {
    let constants = [
        capability::CLOCK_NOW,
        capability::NET_CONNECT,
        capability::NET_BIND,
        capability::FS_READ,
        capability::FS_WRITE,
        capability::IO_STDIN,
        capability::IO_STDOUT,
        capability::IO_STDERR,
        capability::PROCESS_EXEC,
        capability::ENV_READ,
        capability::ENV_WRITE,
        capability::RANDOM_GENERATE,
        capability::LOG_EMIT,
        capability::TRACE_SPAN,
    ];
    for c in constants {
        assert!(
            c.chars().all(|ch| ch.is_lowercase() || ch == '.' || ch.is_ascii_digit()),
            "capability constant {c:?} must be lower-dotted (lowercase + dots only)"
        );
        assert!(!c.is_empty(), "capability constant must not be empty");
        assert!(c.contains('.'), "capability constant must contain a dot: {c:?}");
    }
}

// ── Spec: validate() duplicate-ID detection ───────────────────────────────

#[test]
fn validate_rejects_duplicate_ids() {
    let reg = StdlibRegistry {
        entries: vec![minimal_entry(), minimal_entry()], // two "std.core" entries
    };
    assert!(
        reg.validate().is_err(),
        "registry with duplicate IDs must fail validation"
    );
}

#[test]
fn validate_accepts_unique_ids() {
    let reg = StdlibRegistry {
        entries: vec![
            minimal_entry(),
            StdlibEntry {
                id: StdlibId("std.option".to_string()),
                name: "option".to_string(),
                ..minimal_entry()
            },
        ],
    };
    assert_eq!(
        reg.validate(),
        Ok(()),
        "registry with unique IDs must pass validation"
    );
}
