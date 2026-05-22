// ── ail-stdlib::exec_effectful ────────────────────────────────────────────
//
// Strict TDD — RED phase written BEFORE call_effectful_stdlib exists and
// before std.fs.read_file / missing InMemoryCapabilityHost stubs are added.
//
// Spec scenarios:
//  EFFECTFUL-1: call_effectful_stdlib routes std.fs.read_file to InMemoryCapabilityHost.
//  EFFECTFUL-2: call_effectful_stdlib returns CapabilityRequired when no host given.
//  EFFECTFUL-3: InMemoryCapabilityHost handles network/process/http with stubs.
//  EFFECTFUL-4: call_effectful_stdlib returns UnknownFunction for unregistered IDs.
//  EFFECTFUL-5: std.net.connect routes to network.connect via InMemoryCapabilityHost.

use ail_stdlib::exec::{
    InMemoryCapabilityHost, StdlibCapabilityDispatch, StdlibExecError, StdlibValue,
    call_effectful_stdlib, find_function_entry,
};

// ── EFFECTFUL-1: std.fs.read_file routes to InMemoryCapabilityHost ─────────

#[test]
fn call_effectful_stdlib_routes_fs_read_file_via_host() {
    // RED: call_effectful_stdlib and "std.fs.read_file" entry do not exist yet.
    let host = InMemoryCapabilityHost::new().with_file("/hello.txt", b"world");
    let result = call_effectful_stdlib(
        "std.fs.read_file",
        &[StdlibValue::Text("/hello.txt".to_string())],
        &host,
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Bytes(b"world".to_vec())),
        "std.fs.read_file must read the in-memory file"
    );
}

// ── EFFECTFUL-2: CapabilityRequired when no host is provided ─────────────

#[test]
fn call_effectful_stdlib_returns_capability_required_without_host() {
    // This uses find_function_entry + call() which already returns CapabilityRequired.
    // Proves the convention is consistent.
    let entry = find_function_entry("std.fs.read_file")
        .expect("std.fs.read_file entry must exist");
    let result = entry.call(&[StdlibValue::Text("/any.txt".to_string())]);
    assert!(
        matches!(result, Err(StdlibExecError::CapabilityRequired { .. })),
        "calling without host must return CapabilityRequired, got: {result:?}"
    );
}

// ── EFFECTFUL-3: InMemoryCapabilityHost handles process / http stubs ───────

#[test]
fn in_memory_host_returns_unit_for_process_spawn() {
    // RED: InMemoryCapabilityHost does not handle process.spawn yet.
    let host = InMemoryCapabilityHost::new();
    let result = host.call("process.spawn", "spawn", &[]);
    assert!(
        result.is_ok(),
        "InMemoryCapabilityHost must handle process.spawn with a stub, got: {result:?}"
    );
}

#[test]
fn in_memory_host_returns_stub_for_network_connect() {
    let host = InMemoryCapabilityHost::new();
    let result = host.call("network.connect", "connect", &[]);
    assert!(
        result.is_ok(),
        "InMemoryCapabilityHost must handle network.connect with a stub, got: {result:?}"
    );
}

#[test]
fn in_memory_host_returns_stub_for_http_request() {
    let host = InMemoryCapabilityHost::new();
    let result = host.call("http.call", "request", &[]);
    assert!(
        result.is_ok(),
        "InMemoryCapabilityHost must handle http.call/request with a stub, got: {result:?}"
    );
}

// ── EFFECTFUL-4: UnknownFunction for completely unknown ID ─────────────────

#[test]
fn call_effectful_stdlib_returns_unknown_function_for_unknown_id() {
    let host = InMemoryCapabilityHost::new();
    let result = call_effectful_stdlib("std.does.not.exist", &[], &host);
    assert!(
        matches!(result, Err(StdlibExecError::UnknownFunction(_))),
        "unknown IDs must return UnknownFunction, got: {result:?}"
    );
}

// ── EFFECTFUL-5: std.net.connect routes via InMemoryCapabilityHost ─────────

#[test]
fn call_effectful_stdlib_routes_net_connect_via_host() {
    let host = InMemoryCapabilityHost::new();
    let result = call_effectful_stdlib(
        "std.net.connect",
        &[StdlibValue::Text("tcp://localhost:8080".to_string())],
        &host,
    );
    assert!(
        result.is_ok(),
        "std.net.connect must route through InMemoryCapabilityHost stub, got: {result:?}"
    );
}
