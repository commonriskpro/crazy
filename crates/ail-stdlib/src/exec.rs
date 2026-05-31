// ── ail-stdlib::exec ──────────────────────────────────────────────────────
//
// Executable stdlib function registry.
//
// The metadata registry in `v1` describes the public API shape. This module
// provides the execution-facing table: pure functions carry Rust function
// pointers, while effectful functions carry a capability + operation pair for
// runtime handler dispatch.
//
// # Module layout
//
// | Submodule  | Responsibility |
// |------------|----------------|
// | `types`    | `StdlibValue`, `StdlibExecError`, `PureStdlibFn` |
// | `capability` | `StdlibCapabilityDispatch` trait, `InMemoryCapabilityHost` |
// | `registry` | `FunctionImpl`, `FunctionEntry`, entry table, dispatch API |
// | `registry::handlers` | Pure function implementations (private to registry) |

mod capability;
mod registry;
mod types;

pub use capability::{InMemoryCapabilityHost, StdlibCapabilityDispatch};
pub use registry::{
    FunctionEntry, FunctionImpl, call_effectful_stdlib, call_pure_stdlib, find_function_entry,
    stdlib_function_entries,
};
pub use types::{PureStdlibFn, StdlibExecError, StdlibValue};

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    // `capability` and `pure` builders are pub(super) in registry — accessible
    // from within this module (which is also a child of exec).
    use super::registry::{capability, pure};

    // ── A1: StdlibCapabilityDispatch trait contract ───────────────────────

    // Spec STDLIB-CAP-1:
    //   GIVEN a capability-backed FunctionEntry
    //   WHEN call_with_host() is called with a host
    //   THEN the host is dispatched
    #[test]
    fn dispatch_routes_to_host_when_host_provided() {
        let entry = capability(
            "std.time.now",
            "std.time",
            "now",
            &[],
            "Instant",
            "clock.now",
            "now",
        );
        let host = InMemoryCapabilityHost::new().with_fixed_clock(12345);
        let result = entry.call_with_host(&[], Some(&host));
        assert_eq!(result, Ok(StdlibValue::Int(12345)));
    }

    // Spec STDLIB-CAP-1:
    //   WHEN no host is provided
    //   THEN returns CapabilityRequired error
    #[test]
    fn returns_capability_required_when_no_host() {
        let entry = capability(
            "std.time.now",
            "std.time",
            "now",
            &[],
            "Instant",
            "clock.now",
            "now",
        );
        let result = entry.call_with_host(&[], None);
        assert!(
            matches!(
                result,
                Err(StdlibExecError::CapabilityRequired {
                    ref capability,
                    ref operation,
                }) if capability == "clock.now" && operation == "now"
            ),
            "no host must produce CapabilityRequired"
        );
    }

    // Spec STDLIB-CAP-2: InMemoryCapabilityHost handles clock.now
    #[test]
    fn in_memory_host_clock_now() {
        let host = InMemoryCapabilityHost::new().with_fixed_clock(9_999_999);
        let result = host.call("clock.now", "now", &[]);
        assert_eq!(result, Ok(StdlibValue::Int(9_999_999)));
    }

    // Spec STDLIB-CAP-2: InMemoryCapabilityHost handles env.read.get
    #[test]
    fn in_memory_host_env_read_get() {
        let host = InMemoryCapabilityHost::new().with_env("MY_KEY", "hello");
        let result = host.call(
            "env.read",
            "get",
            &[StdlibValue::Text("MY_KEY".to_string())],
        );
        assert_eq!(
            result,
            Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Text(
                "hello".to_string()
            )))))
        );
    }

    // Spec STDLIB-CAP-2: missing key returns None
    #[test]
    fn in_memory_host_env_read_get_missing() {
        let host = InMemoryCapabilityHost::new();
        let result = host.call(
            "env.read",
            "get",
            &[StdlibValue::Text("MISSING".to_string())],
        );
        assert_eq!(result, Ok(StdlibValue::Option(None)));
    }

    // Spec STDLIB-CAP-2: env.write.set returns Unit
    #[test]
    fn in_memory_host_env_write_set() {
        let host = InMemoryCapabilityHost::new();
        let result = host.call(
            "env.write",
            "set",
            &[
                StdlibValue::Text("K".to_string()),
                StdlibValue::Text("V".to_string()),
            ],
        );
        assert_eq!(result, Ok(StdlibValue::Unit));
    }

    // Spec STDLIB-CAP-2: io.stdout.write returns byte count
    #[test]
    fn in_memory_host_io_stdout_write() {
        let host = InMemoryCapabilityHost::new();
        let result = host.call("io.stdout", "write", &[StdlibValue::Bytes(vec![1u8, 2, 3])]);
        assert_eq!(result, Ok(StdlibValue::Int(3)));
    }

    // Spec STDLIB-CAP-2: file.read.read reads from in-memory file map
    #[test]
    fn in_memory_host_file_read_read() {
        let host = InMemoryCapabilityHost::new().with_file("/data.bin", b"content");
        let result = host.call(
            "file.read",
            "read",
            &[StdlibValue::Text("/data.bin".to_string())],
        );
        assert_eq!(result, Ok(StdlibValue::Bytes(b"content".to_vec())));
    }

    // Pure FunctionEntry: call() still works (backward compat)
    #[test]
    fn pure_entry_call_still_works() {
        let result = call_pure_stdlib("std.text.trim", &[StdlibValue::Text("  hi  ".to_string())]);
        assert_eq!(result, Ok(StdlibValue::Text("hi".to_string())));
    }

    // ── A5: Missing exec entries ──────────────────────────────────────────

    // Spec STDLIB-EXEC-1: std.crypto.hmac
    #[test]
    fn exec_crypto_hmac_entry_exists() {
        let result = call_pure_stdlib(
            "std.crypto.hmac",
            &[
                StdlibValue::Bytes(b"secret-key".to_vec()),
                StdlibValue::Bytes(b"message".to_vec()),
            ],
        );
        assert!(
            matches!(result, Ok(StdlibValue::Bytes(ref b)) if b.len() == 32),
            "hmac must return 32-byte Bytes"
        );
    }

    // Spec STDLIB-EXEC-1: std.crypto.constant_time_eq — equal
    #[test]
    fn exec_crypto_constant_time_eq_equal() {
        let result = call_pure_stdlib(
            "std.crypto.constant_time_eq",
            &[
                StdlibValue::Bytes(b"abc".to_vec()),
                StdlibValue::Bytes(b"abc".to_vec()),
            ],
        );
        assert_eq!(result, Ok(StdlibValue::Bool(true)));
    }

    // Spec STDLIB-EXEC-1: std.crypto.constant_time_eq — not equal
    #[test]
    fn exec_crypto_constant_time_eq_not_equal() {
        let result = call_pure_stdlib(
            "std.crypto.constant_time_eq",
            &[
                StdlibValue::Bytes(b"abc".to_vec()),
                StdlibValue::Bytes(b"xyz".to_vec()),
            ],
        );
        assert_eq!(result, Ok(StdlibValue::Bool(false)));
    }

    // Spec STDLIB-EXEC-1: std.encoding.base64_encode
    #[test]
    fn exec_encoding_base64_encode() {
        let result = call_pure_stdlib(
            "std.encoding.base64_encode",
            &[StdlibValue::Bytes(b"hello".to_vec())],
        );
        // base64("hello") = "aGVsbG8="
        assert_eq!(result, Ok(StdlibValue::Text("aGVsbG8=".to_string())));
    }

    // Spec STDLIB-EXEC-1: std.encoding.base64_decode — success
    #[test]
    fn exec_encoding_base64_decode_ok() {
        let result = call_pure_stdlib(
            "std.encoding.base64_decode",
            &[StdlibValue::Text("aGVsbG8=".to_string())],
        );
        assert_eq!(
            result,
            Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Bytes(
                b"hello".to_vec()
            )))))
        );
    }

    // Spec STDLIB-EXEC-1: std.encoding.base64_decode — error
    #[test]
    fn exec_encoding_base64_decode_err() {
        let result = call_pure_stdlib(
            "std.encoding.base64_decode",
            &[StdlibValue::Text("!!!invalid".to_string())],
        );
        assert!(
            matches!(result, Ok(StdlibValue::Result(Err(_)))),
            "invalid base64 must return Err"
        );
    }

    // Spec STDLIB-EXEC-1: std.encoding.hex_encode
    #[test]
    fn exec_encoding_hex_encode() {
        let result = call_pure_stdlib(
            "std.encoding.hex_encode",
            &[StdlibValue::Bytes(vec![0xca, 0xfe])],
        );
        assert_eq!(result, Ok(StdlibValue::Text("cafe".to_string())));
    }

    // Spec STDLIB-EXEC-1: std.encoding.hex_decode — success
    #[test]
    fn exec_encoding_hex_decode_ok() {
        let result = call_pure_stdlib(
            "std.encoding.hex_decode",
            &[StdlibValue::Text("cafe".to_string())],
        );
        assert_eq!(
            result,
            Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Bytes(vec![
                0xca, 0xfe
            ])))))
        );
    }

    // Spec STDLIB-EXEC-1: std.encoding.hex_decode — error
    #[test]
    fn exec_encoding_hex_decode_err() {
        let result = call_pure_stdlib(
            "std.encoding.hex_decode",
            &[StdlibValue::Text("xyz!".to_string())],
        );
        assert!(
            matches!(result, Ok(StdlibValue::Result(Err(_)))),
            "invalid hex must return Err"
        );
    }

    // Spec STDLIB-EXEC-1: std.json.parse — success
    #[test]
    fn exec_json_parse_ok() {
        let result = call_pure_stdlib(
            "std.json.parse",
            &[StdlibValue::Text(r#"{"x":1}"#.to_string())],
        );
        assert!(
            matches!(result, Ok(StdlibValue::Result(Ok(_)))),
            "valid JSON must return Ok(Map)"
        );
    }

    // Spec STDLIB-EXEC-1: std.json.parse — error
    #[test]
    fn exec_json_parse_err() {
        let result = call_pure_stdlib(
            "std.json.parse",
            &[StdlibValue::Text("not json".to_string())],
        );
        assert!(
            matches!(result, Ok(StdlibValue::Result(Err(_)))),
            "invalid JSON must return Err"
        );
    }

    // Spec STDLIB-EXEC-1: std.json.stringify
    #[test]
    fn exec_json_stringify() {
        let mut map = BTreeMap::new();
        map.insert("k".to_string(), StdlibValue::Int(42));
        let result = call_pure_stdlib("std.json.stringify", &[StdlibValue::Map(map)]);
        assert!(
            matches!(result, Ok(StdlibValue::Text(ref s)) if s.contains("42")),
            "stringify must produce JSON text with value"
        );
    }

    #[test]
    fn exec_json_stringify_tuple_as_array_shape() {
        let result = call_pure_stdlib(
            "std.json.stringify",
            &[StdlibValue::Tuple(vec![
                StdlibValue::Text("front".to_string()),
                StdlibValue::List(vec![StdlibValue::Text("rest".to_string())]),
            ])],
        );
        assert!(
            matches!(result, Ok(StdlibValue::Text(ref s)) if s.contains("front") && s.contains("rest")),
            "tuple stringify must preserve ordered tuple shape as JSON array text"
        );
    }

    // Spec STDLIB-EXEC-1: std.numeric.narrow_to_i32 — ok
    #[test]
    fn exec_numeric_narrow_to_i32_ok() {
        let result = call_pure_stdlib("std.numeric.narrow_to_i32", &[StdlibValue::Int(42)]);
        assert_eq!(
            result,
            Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Int(42)))))
        );
    }

    // Spec STDLIB-EXEC-1: std.numeric.narrow_to_i32 — overflow
    #[test]
    fn exec_numeric_narrow_to_i32_overflow() {
        let result = call_pure_stdlib("std.numeric.narrow_to_i32", &[StdlibValue::Int(i64::MAX)]);
        assert!(
            matches!(result, Ok(StdlibValue::Result(Err(_)))),
            "overflow must return Err"
        );
    }

    // Spec STDLIB-EXEC-1: std.numeric.narrow_to_u32 — ok
    #[test]
    fn exec_numeric_narrow_to_u32_ok() {
        let result = call_pure_stdlib("std.numeric.narrow_to_u32", &[StdlibValue::Int(100)]);
        assert_eq!(
            result,
            Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Int(100)))))
        );
    }

    // Spec STDLIB-EXEC-1: std.numeric.narrow_to_u32 — overflow (negative)
    #[test]
    fn exec_numeric_narrow_to_u32_negative() {
        let result = call_pure_stdlib("std.numeric.narrow_to_u32", &[StdlibValue::Int(-1)]);
        assert!(
            matches!(result, Ok(StdlibValue::Result(Err(_)))),
            "negative value must return Err for u32"
        );
    }

    // ── unused import guard ───────────────────────────────────────────────
    // Ensure `pure` builder imported above is used (silences dead_code lint).
    #[test]
    fn pure_builder_produces_pure_entry() {
        fn dummy(_: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
            Ok(StdlibValue::Unit)
        }
        let entry = pure("test.dummy", "test", "dummy", &[], "Unit", dummy);
        assert_eq!(entry.id, "test.dummy");
    }
}
