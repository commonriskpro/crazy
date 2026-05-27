use super::helpers::*;

#[test]
fn native_div_differs_from_placeholder() {
    let art = emit_native(&anf_with_call2("i64.div_s", 10, 2)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "i64.div_s must produce different bytes than Placeholder"
    );
}

#[test]
fn native_rem_differs_from_placeholder() {
    let art = emit_native(&anf_with_call2("i64.rem_s", 10, 3)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "i64.rem_s must produce different bytes than Placeholder"
    );
}

#[test]
fn native_eq_differs_from_placeholder() {
    let art = emit_native(&anf_with_call2("i64.eq", 5, 5)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "i64.eq must produce different bytes than Placeholder"
    );
}

#[test]
fn native_neg_differs_from_placeholder() {
    let art = emit_native(&anf_with_call1("i64.neg", 7)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "i64.neg must produce different bytes than Placeholder"
    );
}

#[test]
fn native_eqz_differs_from_placeholder() {
    let art = emit_native(&anf_with_call1("i64.eqz", 0)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "i64.eqz must produce different bytes than Placeholder"
    );
}

// ── TASK-B0: If + ShortCircuit tests — RED ────────────────────────────
// These hit the catch-all `_ =>` trap arm until B1 lands.
