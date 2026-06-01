// Tests for extended v1_registry — Function-kind entries for G26 stdlib-impl.
//
// TDD cycle: written before v1.rs is extended.
// Spec: G26 stdlib-impl, Requirement R6.1–R6.3.

use ail_core::semantic_graph::{NodeKind, SemanticGraph};
use ail_stdlib::registry::StdlibRegistry;
use ail_stdlib::{v1_registry, v1_registry_with_functions};

// ── R6.1: Function entries exist in extended registry ─────────────────────

// v1_registry_with_functions() must contain Function-kind entries for each module
#[test]
fn v1_functions_registry_contains_function_entries() {
    let reg = v1_registry_with_functions();
    let function_entries: Vec<_> = reg
        .entries
        .iter()
        .filter(|e| e.kind == NodeKind::Function)
        .collect();
    assert!(
        !function_entries.is_empty(),
        "registry must contain at least one Function-kind entry"
    );
}

// Each of the five modules must have at least one Function entry
#[test]
fn v1_functions_numeric_has_checked_add() {
    let reg = v1_registry_with_functions();
    assert!(
        reg.entries
            .iter()
            .any(|e| e.id.0 == "std.numeric.checked_add"),
        "registry must contain std.numeric.checked_add"
    );
}

#[test]
fn v1_functions_numeric_has_bounds_helpers() {
    let reg = v1_registry_with_functions();
    for id in ["std.numeric.min", "std.numeric.max", "std.numeric.clamp"] {
        assert!(
            reg.entries.iter().any(|e| e.id.0 == id),
            "registry must contain {id}"
        );
    }
}

#[test]
fn v1_functions_option_has_map() {
    let reg = v1_registry_with_functions();
    assert!(
        reg.entries.iter().any(|e| e.id.0 == "std.core.option.map"),
        "registry must contain std.core.option.map"
    );
}

#[test]
fn v1_functions_result_has_map() {
    let reg = v1_registry_with_functions();
    assert!(
        reg.entries.iter().any(|e| e.id.0 == "std.core.result.map"),
        "registry must contain std.core.result.map"
    );
}

#[test]
fn v1_functions_text_has_trim() {
    let reg = v1_registry_with_functions();
    assert!(
        reg.entries.iter().any(|e| e.id.0 == "std.text.trim"),
        "registry must contain std.text.trim"
    );
}

#[test]
fn v1_functions_iter_has_traverse() {
    let reg = v1_registry_with_functions();
    assert!(
        reg.entries.iter().any(|e| e.id.0 == "std.iter.traverse"),
        "registry must contain std.iter.traverse"
    );
}

// ── R6.2: Function entries have type_facts ────────────────────────────────

#[test]
fn v1_function_entries_have_type_facts() {
    let reg = v1_registry_with_functions();
    for entry in reg.entries.iter().filter(|e| e.kind == NodeKind::Function) {
        assert!(
            entry.type_facts.is_some(),
            "Function entry {:?} must have type_facts",
            entry.id.0
        );
    }
}

// ── R6.3: validate() passes after adding Function entries ─────────────────

#[test]
fn v1_functions_registry_validates() {
    let reg = v1_registry_with_functions();
    assert_eq!(
        reg.validate(),
        Ok(()),
        "extended registry must pass validate() (no duplicate IDs)"
    );
}

// All projected nodes pass SemanticGraph::validate()
#[test]
fn v1_functions_registry_projected_nodes_valid() {
    let reg = v1_registry_with_functions();
    let nodes = reg.to_graph_nodes();
    let graph = SemanticGraph {
        nodes,
        edges: vec![],
    };
    assert_eq!(
        graph.validate(),
        Ok(()),
        "extended registry projected nodes must pass SemanticGraph::validate()"
    );
}

// CBOR round-trip with Function entries
#[test]
fn v1_functions_registry_cbor_round_trip() {
    let reg = v1_registry_with_functions();
    let bytes = reg.cbor_bytes().expect("cbor_bytes must succeed");
    let decoded = StdlibRegistry::from_cbor_bytes(&bytes).expect("from_cbor_bytes must succeed");
    assert_eq!(
        decoded, reg,
        "extended registry must survive CBOR round-trip"
    );
}

// Extended registry has more entries than base registry
#[test]
fn v1_functions_registry_has_more_entries_than_base() {
    let base = v1_registry();
    let ext = v1_registry_with_functions();
    assert!(
        ext.entries.len() > base.entries.len(),
        "extended registry must have more entries than the 9-module base"
    );
}
