// ── ail-runtime::abi_descriptor_tests ────────────────────────────────────
//
// Locks the ABI versioning contract, Text layout decoding, and Handle
// reference-count lifecycle.
//
// Coverage:
//  ABI-V1 — ABI_VERSION constant is 1.
//  ABI-V2 — AbiDescriptor::new wraps exports at the current version.
//  ABI-V3 — AbiDescriptor::is_compatible rejects wrong versions.
//  ABI-V4 — AbiDescriptor round-trips through serde_json.
//  TEXT-1 — ValueLayout::Text decodes packed i64 into StructuredValue::Text.
//  TEXT-2 — Zero ptr and zero len decode correctly.
//  TEXT-3 — Negative packed values (sign bits set) decode ptr/len correctly.
//  RC-1   — HandleRegistry::create starts at count 1 (contains == true).
//  RC-2   — clone_handle increments count; handle survives first release.
//  RC-3   — Two releases after one clone returns true on the second release.
//  RC-4   — clone_handle on a released handle returns false.
//  RC-5   — release on a never-created handle returns false.

use std::collections::BTreeMap;

use ail_compiler::wasm::{ABI_VERSION, AbiDescriptor, WasmScalarType, WasmTypeDescriptor};
use ail_runtime::{HandleId, HandleRegistry, StructuredValue, ValueDecoder, ValueLayout};

// ── ABI-V1 ────────────────────────────────────────────────────────────────

#[test]
fn abi_version_is_one() {
    assert_eq!(
        ABI_VERSION, 1,
        "ABI_VERSION must be 1 for the initial release"
    );
}

// ── ABI-V2 ────────────────────────────────────────────────────────────────

#[test]
fn abi_descriptor_new_sets_current_version() {
    let exports = BTreeMap::new();
    let desc = AbiDescriptor::new(exports);
    assert_eq!(
        desc.abi_version, ABI_VERSION,
        "AbiDescriptor::new must stamp abi_version with ABI_VERSION"
    );
}

// ── ABI-V3 ────────────────────────────────────────────────────────────────

#[test]
fn abi_descriptor_is_compatible_rejects_wrong_version() {
    let mut desc = AbiDescriptor::new(BTreeMap::new());
    assert!(desc.is_compatible(), "current version must be compatible");

    desc.abi_version = 0;
    assert!(!desc.is_compatible(), "version 0 must not be compatible");

    desc.abi_version = 2;
    assert!(
        !desc.is_compatible(),
        "version 2 must not be compatible yet"
    );
}

// ── ABI-V4 ────────────────────────────────────────────────────────────────

#[test]
fn abi_descriptor_roundtrips_via_serde_json() {
    let mut exports = BTreeMap::new();
    exports.insert(
        "add".to_string(),
        WasmTypeDescriptor::Scalar(WasmScalarType::I64),
    );
    exports.insert("name".to_string(), WasmTypeDescriptor::Text);
    exports.insert(
        "pos".to_string(),
        WasmTypeDescriptor::Record {
            fields: vec!["x".to_string(), "y".to_string()],
        },
    );

    let desc = AbiDescriptor::new(exports);
    let json = serde_json::to_string(&desc).expect("serialize must succeed");
    let restored: AbiDescriptor = serde_json::from_str(&json).expect("deserialize must succeed");

    assert_eq!(
        desc, restored,
        "AbiDescriptor must survive serde_json round-trip"
    );
    assert!(
        restored.is_compatible(),
        "restored descriptor must be compatible"
    );
    assert_eq!(
        restored.exports.get("name"),
        Some(&WasmTypeDescriptor::Text)
    );
}

// ── TEXT-1 ────────────────────────────────────────────────────────────────

#[test]
fn value_layout_text_decodes_packed_i64_to_structured_text() {
    // ptr = 0x80, len = 5 → packed = (5 << 32) | 0x80
    let ptr: i32 = 0x80;
    let len: i32 = 5;
    let packed: i64 = ((len as i64) << 32) | (ptr as i64);

    let result = ValueDecoder::decode(&ValueLayout::Text, packed, &[]);
    assert_eq!(
        result,
        StructuredValue::Text { ptr, len },
        "ValueLayout::Text must unpack packed i64 into StructuredValue::Text"
    );
}

// ── TEXT-2 ────────────────────────────────────────────────────────────────

#[test]
fn value_layout_text_zero_ptr_zero_len() {
    let result = ValueDecoder::decode(&ValueLayout::Text, 0, &[]);
    assert_eq!(
        result,
        StructuredValue::Text { ptr: 0, len: 0 },
        "packed 0 must yield Text {{ ptr: 0, len: 0 }}"
    );
}

// ── TEXT-3 ────────────────────────────────────────────────────────────────

#[test]
fn value_layout_text_large_ptr_and_len() {
    // Typical case: ptr = 4096, len = 42
    let ptr: i32 = 4096;
    let len: i32 = 42;
    let packed: i64 = ((len as i64) << 32) | (ptr as i64 & 0xFFFF_FFFF);
    let result = ValueDecoder::decode(&ValueLayout::Text, packed, &[]);
    assert_eq!(result, StructuredValue::Text { ptr, len });
}

// ── RC-1 ──────────────────────────────────────────────────────────────────

#[test]
fn handle_rc_create_starts_at_count_one() {
    let mut reg = HandleRegistry::new();
    let id = reg.create();
    assert!(
        reg.contains(id),
        "handle must be active immediately after create"
    );
}

// ── RC-2 ──────────────────────────────────────────────────────────────────

#[test]
fn handle_rc_clone_then_one_release_keeps_handle_active() {
    let mut reg = HandleRegistry::new();
    let id = reg.create(); // ref count = 1

    let cloned = reg.clone_handle(id); // ref count = 2
    assert!(cloned, "clone_handle must return true for an active handle");
    assert!(reg.contains(id), "handle must still be active after clone");

    let fully_released = reg.release(id); // ref count = 1
    assert!(
        !fully_released,
        "first release after clone must not fully release the handle"
    );
    assert!(
        reg.contains(id),
        "handle must still be active after first of two releases"
    );
}

// ── RC-3 ──────────────────────────────────────────────────────────────────

#[test]
fn handle_rc_two_releases_after_one_clone_fully_releases() {
    let mut reg = HandleRegistry::new();
    let id = reg.create(); // count = 1
    reg.clone_handle(id); // count = 2

    let first = reg.release(id); // count = 1 → not fully released
    let second = reg.release(id); // count = 0 → fully released

    assert!(!first, "first release must return false (count 2→1)");
    assert!(second, "second release must return true (count 1→0)");
    assert!(
        !reg.contains(id),
        "handle must be inactive after both releases"
    );
}

// ── RC-4 ──────────────────────────────────────────────────────────────────

#[test]
fn handle_rc_clone_on_released_handle_returns_false() {
    let mut reg = HandleRegistry::new();
    let id = reg.create();
    reg.release(id); // count = 0

    let cloned = reg.clone_handle(id);
    assert!(
        !cloned,
        "clone_handle on a fully released handle must return false"
    );
    assert!(
        !reg.contains(id),
        "handle must remain inactive after failed clone"
    );
}

// ── RC-5 ──────────────────────────────────────────────────────────────────

#[test]
fn handle_rc_release_on_unknown_id_returns_false() {
    let mut reg = HandleRegistry::new();
    let phantom = HandleId(999);
    assert!(
        !reg.release(phantom),
        "release on a never-created handle must return false"
    );
    assert!(
        !reg.contains(phantom),
        "contains on a never-created handle must return false"
    );
}
