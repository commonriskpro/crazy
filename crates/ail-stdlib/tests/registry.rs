// Integration tests for ail-stdlib (PR 1 + PR 2 slices).
//
// Covers spec scenarios: StabilityTier, CBOR round-trips, deterministic hash,
// graph projection infrastructure, capability constant contracts, and the
// canonical v1 registry (scenarios 4.6–4.10 added in PR 2).

use ail_core::semantic_graph::{
    CapabilityReqs, ContractClauses, EffectRow, NodeKind, NodeRef, SemanticGraph, TypeFacts,
};
use ail_stdlib::{
    capability,
    registry::{StabilityTier, StdlibEntry, StdlibId, StdlibRegistry},
    v1_registry,
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
            c.chars()
                .all(|ch| ch.is_lowercase() || ch == '.' || ch.is_ascii_digit()),
            "capability constant {c:?} must be lower-dotted (lowercase + dots only)"
        );
        assert!(!c.is_empty(), "capability constant must not be empty");
        assert!(
            c.contains('.'),
            "capability constant must contain a dot: {c:?}"
        );
    }
}

// ── Spec: v1 registry — scenarios 4.6–4.10 ───────────────────────────────
//
// These tests exercise `v1_registry()` — added in PR 2.

// Spec scenario 4.6: Projected nodes pass graph validation
//   GIVEN the default v1 StdlibRegistry
//   WHEN to_graph_nodes() is called and all nodes are inserted into an empty SemanticGraph
//   THEN SemanticGraph::validate() returns Ok(())
#[test]
fn v1_projected_nodes_pass_semantic_graph_validation() {
    let reg = v1_registry();
    let nodes = reg.to_graph_nodes();
    let graph = SemanticGraph {
        nodes,
        edges: vec![],
    };
    assert_eq!(
        graph.validate(),
        Ok(()),
        "v1 projected nodes must pass SemanticGraph::validate()"
    );
}

// Spec scenario 4.7: All projected NodeRef values are unique
//   GIVEN a StdlibRegistry with N entries
//   WHEN to_graph_nodes() is called
//   THEN every returned GraphNode carries a distinct NodeRef
#[test]
fn v1_projected_node_refs_are_unique() {
    let reg = v1_registry();
    let nodes = reg.to_graph_nodes();
    let mut refs: Vec<NodeRef> = nodes.iter().map(|n| n.id).collect();
    let original_len = refs.len();
    refs.sort();
    refs.dedup();
    assert_eq!(
        refs.len(),
        original_len,
        "all projected NodeRef values must be unique"
    );
}

// Spec scenario 4.8: NodeKind is preserved from StdlibEntry
//   GIVEN a StdlibEntry with kind = NodeKind::Capability
//   WHEN projected via to_graph_nodes()
//   THEN the resulting GraphNode has kind = NodeKind::Capability
#[test]
fn v1_node_kind_preserved_through_projection() {
    let reg = v1_registry();
    // std.capability is the last (index 8) and is NodeKind::Capability
    let capability_entry = reg
        .entries
        .iter()
        .find(|e| e.id.0 == "std.capability")
        .expect("std.capability must be present in v1 registry");
    assert_eq!(
        capability_entry.kind,
        NodeKind::Capability,
        "std.capability entry must have NodeKind::Capability"
    );

    let nodes = reg.to_graph_nodes();
    let capability_node = nodes
        .iter()
        .find(|n| n.name == "std.capability")
        .expect("projected std.capability node must exist");
    assert_eq!(
        capability_node.kind,
        NodeKind::Capability,
        "projected std.capability node must preserve NodeKind::Capability"
    );
}

// Spec scenario 4.9: Entry count equals 9
//   GIVEN the default v1 StdlibRegistry
//   WHEN entries are counted
//   THEN count equals 9
#[test]
fn v1_registry_contains_exactly_9_entries() {
    let reg = v1_registry();
    assert_eq!(
        reg.entries.len(),
        9,
        "v1 registry must contain exactly 9 entries"
    );
}

// Spec scenario 4.10: Out-of-scope modules are absent
//   GIVEN the default v1 StdlibRegistry
//   WHEN searched for std.fs or std.net entries
//   THEN neither entry is present
#[test]
fn v1_registry_excludes_out_of_scope_modules() {
    let reg = v1_registry();
    let ids: Vec<&str> = reg.entries.iter().map(|e| e.id.0.as_str()).collect();
    assert!(
        !ids.contains(&"std.fs"),
        "std.fs must not be present in the v1 registry"
    );
    assert!(
        !ids.contains(&"std.net"),
        "std.net must not be present in the v1 registry"
    );
}

// ── Spec: stdlib-types (G11) — rich metadata scenarios S1–S10 ────────────
//
// These tests verify that all 9 v1 entries have populated type_facts,
// and that effect_row / capability_reqs / contract_clauses are present
// where the spec requires them.

// S1: all 9 entries have type_facts populated
#[test]
fn v1_all_entries_have_type_facts() {
    let reg = v1_registry();
    for entry in &reg.entries {
        assert!(
            entry.type_facts.is_some(),
            "entry {:?} must have type_facts populated",
            entry.id.0
        );
    }
}

// S2: std.option type_facts has generics ["T"]
#[test]
fn v1_option_type_facts_generic_t() {
    let reg = v1_registry();
    let entry = reg
        .entries
        .iter()
        .find(|e| e.id.0 == "std.option")
        .expect("std.option must be present");
    let tf = entry.type_facts.as_ref().expect("type_facts must be Some");
    assert_eq!(tf.nominal, "Option");
    assert_eq!(tf.generics, vec!["T"]);
}

// S3: std.result type_facts has generics ["T", "E"]
#[test]
fn v1_result_type_facts_generics_t_e() {
    let reg = v1_registry();
    let entry = reg
        .entries
        .iter()
        .find(|e| e.id.0 == "std.result")
        .expect("std.result must be present");
    let tf = entry.type_facts.as_ref().expect("type_facts must be Some");
    assert_eq!(tf.nominal, "Result");
    assert_eq!(tf.generics, vec!["T", "E"]);
}

// S4: std.numeric contract_clauses contains "no silent overflow"
#[test]
fn v1_numeric_contract_no_silent_overflow() {
    let reg = v1_registry();
    let entry = reg
        .entries
        .iter()
        .find(|e| e.id.0 == "std.numeric")
        .expect("std.numeric must be present");
    let cc = entry
        .contract_clauses
        .as_ref()
        .expect("std.numeric must have contract_clauses");
    assert!(
        cc.requires.iter().any(|r| r.contains("no silent overflow")),
        "std.numeric requires must contain 'no silent overflow'; got: {:?}",
        cc.requires
    );
}

// S5: std.collections contract_clauses contains "length >= 0"
#[test]
fn v1_collections_contract_length_ge_zero() {
    let reg = v1_registry();
    let entry = reg
        .entries
        .iter()
        .find(|e| e.id.0 == "std.collections")
        .expect("std.collections must be present");
    let cc = entry
        .contract_clauses
        .as_ref()
        .expect("std.collections must have contract_clauses");
    assert!(
        cc.requires.iter().any(|r| r.contains("length >= 0")),
        "std.collections requires must contain 'length >= 0'; got: {:?}",
        cc.requires
    );
}

// S6: std.text contract_clauses contains "valid UTF-8 input"
#[test]
fn v1_text_contract_valid_utf8() {
    let reg = v1_registry();
    let entry = reg
        .entries
        .iter()
        .find(|e| e.id.0 == "std.text")
        .expect("std.text must be present");
    let cc = entry
        .contract_clauses
        .as_ref()
        .expect("std.text must have contract_clauses");
    assert!(
        cc.requires.iter().any(|r| r.contains("valid UTF-8")),
        "std.text requires must contain 'valid UTF-8 input'; got: {:?}",
        cc.requires
    );
}

// S7: std.iter has effect_row with "EffectPoly"
#[test]
fn v1_iter_effect_row_effect_poly() {
    let reg = v1_registry();
    let entry = reg
        .entries
        .iter()
        .find(|e| e.id.0 == "std.iter")
        .expect("std.iter must be present");
    let er = entry
        .effect_row
        .as_ref()
        .expect("std.iter must have effect_row");
    assert!(
        er.effects.iter().any(|e| e == "EffectPoly"),
        "std.iter effect_row must contain 'EffectPoly'; got: {:?}",
        er.effects
    );
}

// S8: std.capability has both effect_row and capability_reqs populated
#[test]
fn v1_capability_has_effect_row_and_capability_reqs() {
    let reg = v1_registry();
    let entry = reg
        .entries
        .iter()
        .find(|e| e.id.0 == "std.capability")
        .expect("std.capability must be present");
    assert!(
        entry.effect_row.is_some(),
        "std.capability must have effect_row"
    );
    assert!(
        entry.capability_reqs.is_some(),
        "std.capability must have capability_reqs"
    );
}

// S9: v1_registry CBOR round-trip with rich metadata
#[test]
fn v1_registry_rich_metadata_cbor_round_trip() {
    let reg = v1_registry();
    let bytes = reg.cbor_bytes().expect("cbor_bytes must succeed");
    let decoded = StdlibRegistry::from_cbor_bytes(&bytes).expect("from_cbor_bytes must succeed");
    assert_eq!(
        decoded, reg,
        "v1 registry must survive CBOR round-trip with rich metadata"
    );
}

// S10: std.capability capability_reqs contains all 14 capability constants
#[test]
fn v1_capability_reqs_contains_all_14_constants() {
    use ail_stdlib::capability;
    let reg = v1_registry();
    let entry = reg
        .entries
        .iter()
        .find(|e| e.id.0 == "std.capability")
        .expect("std.capability must be present");
    let reqs = entry
        .capability_reqs
        .as_ref()
        .expect("std.capability must have capability_reqs");

    let expected = [
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

    assert_eq!(
        reqs.caps.len(),
        expected.len(),
        "std.capability must declare exactly {} capability constants",
        expected.len()
    );

    for cap in expected {
        assert!(
            reqs.caps.iter().any(|c| c == cap),
            "std.capability capability_reqs must contain {:?}",
            cap
        );
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
