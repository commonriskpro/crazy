use super::helpers::*;

#[test]
fn build_type_section_none_for_zero() {
    assert!(build_type_section(&[], false, false).is_none());
}

// Scenario: build_type_section returns Some when needs_fold is true even with 0 signatures.
#[test]
fn build_type_section_some_when_needs_fold() {
    assert!(build_type_section(&[], true, false).is_some());
}

// TRIANGULATE: build_type_section returns Some for N > 0.
#[test]
fn build_type_section_some_for_nonzero() {
    let signature = WasmSignature {
        param_count: 0,
        result: None,
    };
    assert!(build_type_section(std::slice::from_ref(&signature), false, false).is_some());
    assert!(build_type_section(&vec![signature; 5], false, false).is_some());
}
