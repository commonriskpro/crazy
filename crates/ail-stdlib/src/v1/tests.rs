use super::*;
use ail_core::semantic_graph::NodeKind;

fn has_function_entry(id: &str) -> bool {
    let reg = v1_registry_with_functions();
    reg.entries
        .iter()
        .any(|e| e.id.0 == id && e.kind == NodeKind::Function)
}

fn has_capability_effect(id: &str) -> bool {
    let reg = v1_registry_with_functions();
    reg.entries.iter().any(|e| {
        e.id.0 == id
            && e.kind == NodeKind::Function
            && e.effect_row.is_some()
            && e.capability_reqs.is_some()
    })
}

fn has_contract_clauses(id: &str) -> bool {
    let reg = v1_registry_with_functions();
    reg.entries
        .iter()
        .any(|e| e.id.0 == id && e.contract_clauses.is_some())
}

// Wave 17B: ok_or contract clauses survive the dedup loop
#[test]
fn v1_ok_or_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.core.option.ok_or"),
        "std.core.option.ok_or must have contract_clauses (pre-loop entry required)"
    );
}

// A7: crypto pure function entries
#[test]
fn v1_contains_crypto_hash() {
    assert!(
        has_function_entry("std.crypto.hash"),
        "std.crypto.hash must be present"
    );
}

#[test]
fn v1_contains_crypto_hmac() {
    assert!(
        has_function_entry("std.crypto.hmac"),
        "std.crypto.hmac must be present"
    );
}

#[test]
fn v1_contains_crypto_constant_time_eq() {
    assert!(has_function_entry("std.crypto.constant_time_eq"));
}

// A7: encoding pure function entries
#[test]
fn v1_contains_encoding_base64_encode() {
    assert!(has_function_entry("std.encoding.base64_encode"));
}

#[test]
fn v1_contains_encoding_base64_decode() {
    assert!(has_function_entry("std.encoding.base64_decode"));
}

#[test]
fn v1_contains_encoding_hex_encode() {
    assert!(has_function_entry("std.encoding.hex_encode"));
}

#[test]
fn v1_contains_encoding_hex_decode() {
    assert!(has_function_entry("std.encoding.hex_decode"));
}

// A7: json pure function entries
#[test]
fn v1_contains_json_parse() {
    assert!(has_function_entry("std.json.parse"));
}

#[test]
fn v1_contains_json_stringify() {
    assert!(has_function_entry("std.json.stringify"));
}

// A7: numeric narrowing entries
#[test]
fn v1_contains_numeric_narrow_to_i32() {
    assert!(has_function_entry("std.numeric.narrow_to_i32"));
}

#[test]
fn v1_contains_numeric_narrow_to_u32() {
    assert!(has_function_entry("std.numeric.narrow_to_u32"));
}

// A7: capability (effectful) entries for io
#[test]
fn v1_contains_io_read_with_effect() {
    assert!(
        has_capability_effect("std.io.read"),
        "std.io.read must have effect_row and capability_reqs"
    );
}

#[test]
fn v1_contains_io_write_with_effect() {
    assert!(has_capability_effect("std.io.write"));
}

#[test]
fn v1_contains_io_flush_with_effect() {
    assert!(has_capability_effect("std.io.flush"));
}

// A7: capability entries for fs
#[test]
fn v1_contains_fs_open_with_effect() {
    assert!(has_capability_effect("std.fs.open"));
}

#[test]
fn v1_contains_fs_read_with_effect() {
    assert!(has_capability_effect("std.fs.read"));
}

#[test]
fn v1_contains_fs_write_with_effect() {
    assert!(has_capability_effect("std.fs.write"));
}

// A7: capability entries for env
#[test]
fn v1_contains_env_get_with_effect() {
    assert!(has_capability_effect("std.env.get"));
}

#[test]
fn v1_contains_env_set_with_effect() {
    assert!(has_capability_effect("std.env.set"));
}

// A7: capability entries for log and trace
#[test]
fn v1_contains_log_log_with_effect() {
    assert!(has_capability_effect("std.log.log"));
}

#[test]
fn v1_contains_trace_span_with_effect() {
    assert!(has_capability_effect("std.trace.span"));
}

#[test]
fn v1_contains_trace_event_with_effect() {
    assert!(has_capability_effect("std.trace.event"));
}

// Wave 21D: std.collections list/map/set contract clauses
#[test]
fn v1_list_length_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.collections.list.length"),
        "std.collections.list.length must have contract_clauses (pre-loop entry required)"
    );
}

#[test]
fn v1_list_push_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.collections.list.push"),
        "std.collections.list.push must have contract_clauses"
    );
}

#[test]
fn v1_list_get_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.collections.list.get"),
        "std.collections.list.get must have contract_clauses"
    );
}

#[test]
fn v1_list_map_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.collections.list.map"),
        "std.collections.list.map must have contract_clauses"
    );
}

#[test]
fn v1_list_filter_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.collections.list.filter"),
        "std.collections.list.filter must have contract_clauses"
    );
}

#[test]
fn v1_list_fold_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.collections.list.fold"),
        "std.collections.list.fold must have contract_clauses"
    );
}

#[test]
fn v1_list_concat_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.collections.list.concat"),
        "std.collections.list.concat must have contract_clauses"
    );
}

#[test]
fn v1_map_get_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.collections.map.get"),
        "std.collections.map.get must have contract_clauses"
    );
}

#[test]
fn v1_map_insert_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.collections.map.insert"),
        "std.collections.map.insert must have contract_clauses"
    );
}

#[test]
fn v1_set_contains_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.collections.set.contains"),
        "std.collections.set.contains must have contract_clauses"
    );
}

#[test]
fn v1_set_insert_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.collections.set.insert"),
        "std.collections.set.insert must have contract_clauses"
    );
}

// Wave 21D: std.time contract clauses
#[test]
fn v1_time_duration_since_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.time.duration_since"),
        "std.time.duration_since must have contract_clauses"
    );
}

#[test]
fn v1_time_add_duration_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.time.add_duration"),
        "std.time.add_duration must have contract_clauses"
    );
}

#[test]
fn v1_time_instant_to_ms_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.time.instant_to_ms"),
        "std.time.instant_to_ms must have contract_clauses"
    );
}

#[test]
fn v1_time_now_has_contract_clauses() {
    assert!(
        has_contract_clauses("std.time.now"),
        "std.time.now must have contract_clauses (pre-loop entry required)"
    );
}

#[test]
fn v1_time_now_has_capability_effect() {
    assert!(
        has_capability_effect("std.time.now"),
        "std.time.now must have effect_row and capability_reqs (clock.now)"
    );
}

// Wave 18C: text predicate entries
#[test]
fn v1_contains_text_starts_with() {
    assert!(
        has_function_entry("std.text.starts_with"),
        "std.text.starts_with must be present"
    );
}

#[test]
fn v1_contains_text_ends_with() {
    assert!(
        has_function_entry("std.text.ends_with"),
        "std.text.ends_with must be present"
    );
}

#[test]
fn v1_contains_text_contains() {
    assert!(
        has_function_entry("std.text.contains"),
        "std.text.contains must be present"
    );
}

#[test]
fn v1_contains_text_replace() {
    assert!(
        has_function_entry("std.text.replace"),
        "std.text.replace must be present"
    );
}
