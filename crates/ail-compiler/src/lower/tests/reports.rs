use super::*;

// ── is_report_accepted ────────────────────────────────────────────────

// Proven and RuntimeChecked are accepted; all others are rejected.
#[test]
fn proven_is_accepted() {
    assert!(is_report_accepted(&proven_report()));
}

#[test]
fn runtime_checked_is_accepted() {
    let report = report_with_state(VerificationState::RuntimeChecked);
    assert!(is_report_accepted(&report));
}

#[test]
fn failed_is_rejected() {
    let report = report_with_state(VerificationState::Failed);
    assert!(!is_report_accepted(&report));
}

#[test]
fn assumed_is_rejected() {
    let report = report_with_state(VerificationState::Assumed);
    assert!(!is_report_accepted(&report));
}

#[test]
fn unverified_is_rejected() {
    let report = report_with_state(VerificationState::Unverified);
    assert!(!is_report_accepted(&report));
}

#[test]
fn unsafe_is_rejected() {
    let report = report_with_state(VerificationState::Unsafe);
    assert!(!is_report_accepted(&report));
}

// ── map_node_kind ─────────────────────────────────────────────────────

// All 10 source kinds map to their CoreNodeKind counterpart.
#[test]
fn all_node_kinds_map_correctly() {
    use crate::core_ir::CoreNodeKind;
    let cases = [
        (NodeKind::Module, CoreNodeKind::Module),
        (NodeKind::Function, CoreNodeKind::Function),
        (NodeKind::Type, CoreNodeKind::Type),
        (NodeKind::Effect, CoreNodeKind::Effect),
        (NodeKind::Capability, CoreNodeKind::Capability),
        (NodeKind::Contract, CoreNodeKind::Contract),
        (NodeKind::Invariant, CoreNodeKind::Invariant),
        (NodeKind::Test, CoreNodeKind::Test),
        (NodeKind::Boundary, CoreNodeKind::Boundary),
        (NodeKind::Package, CoreNodeKind::Package),
    ];
    for (src, expected) in cases {
        assert_eq!(
            map_node_kind(src),
            expected,
            "NodeKind::{src:?} must map to CoreNodeKind::{expected:?}"
        );
    }
}

// ── map_core_node_to_anf ──────────────────────────────────────────────

// Provenance and name are preserved verbatim.
#[test]
fn map_core_node_to_anf_preserves_source_ref_and_name() {
    use crate::core_ir::{CoreNode, CoreNodeKind};
    let node = CoreNode {
        source_ref: NodeRef(7),
        kind: CoreNodeKind::Function,
        name: "fn_x".to_string(),
        ty: None,
        expr: None,
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    map_core_node_to_anf(&node, &mut fresh, &mut out);
    let binding = out.into_iter().next().expect("must produce one binding");
    assert_eq!(binding.source_ref, NodeRef(7));
    assert_eq!(binding.name, "fn_x");
}
