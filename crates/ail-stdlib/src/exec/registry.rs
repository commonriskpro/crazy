// ── ail-stdlib::exec::registry ────────────────────────────────────────────
//
// Function descriptors, the full v1 entry table, and the public dispatch API.
//
// `handlers` is a private child module — its functions are `pub(super)` and
// brought into this module's scope via `use self::handlers::*`.

mod handlers;

use self::handlers::{
    bytes_at, bytes_concat, bytes_empty, bytes_length, bytes_slice, concurrent_channel_len,
    concurrent_channel_new, concurrent_channel_recv, concurrent_channel_send,
    crypto_constant_time_eq, crypto_hash, crypto_hmac, encoding_base64_decode,
    encoding_base64_encode, encoding_hex_decode, encoding_hex_encode, iter_all_exec, iter_any_exec,
    iter_filter_exec, iter_find_exec, iter_fold_exec, iter_map_exec, iter_position_exec,
    iter_traverse_exec, json_parse, json_stringify, list_concat_exec, list_filter_exec,
    list_fold_exec, list_get, list_is_empty, list_length, list_map_exec, list_push,
    map_contains_key, map_get, map_insert, map_length, numeric_checked_add, numeric_checked_mul,
    numeric_checked_sub, numeric_narrow_to_i32, numeric_narrow_to_u32, numeric_saturating_add,
    numeric_wrapping_add, option_and_then, option_collect_results, option_map, option_ok_or,
    option_transpose, option_unwrap_or, result_and_then, result_map, result_transpose,
    result_unwrap_or, set_contains, set_insert, set_length, text_contains_exec, text_decode,
    text_encode, text_ends_with_exec, text_format, text_join, text_length_graphemes_exec,
    text_normalize, text_regex, text_replace_exec, text_split, text_starts_with_exec, text_trim,
    time_add_duration_exec, time_duration_since_exec, time_instant_to_ms_exec,
};

use super::capability::StdlibCapabilityDispatch;
use super::types::{PureStdlibFn, StdlibExecError, StdlibValue};

// ── FunctionImpl ──────────────────────────────────────────────────────────

/// Executable implementation behind a stdlib function entry.
#[derive(Clone, Copy)]
pub enum FunctionImpl {
    Pure(PureStdlibFn),
    Capability {
        capability: &'static str,
        operation: &'static str,
    },
}

// ── FunctionEntry ─────────────────────────────────────────────────────────

/// Runtime-facing stdlib function descriptor.
#[derive(Clone, Copy)]
pub struct FunctionEntry {
    pub id: &'static str,
    pub module: &'static str,
    pub name: &'static str,
    pub params: &'static [&'static str],
    pub return_type: &'static str,
    pub implementation: FunctionImpl,
}

impl FunctionEntry {
    /// Execute this function, optionally routing capability calls through `host`.
    ///
    /// For pure functions, `host` is ignored.
    /// For capability-backed functions:
    /// - If `host` is `Some`, dispatches to `host.call(capability, operation, args)`.
    /// - If `host` is `None`, returns `CapabilityRequired`.
    pub fn call_with_host(
        &self,
        args: &[StdlibValue],
        host: Option<&dyn StdlibCapabilityDispatch>,
    ) -> Result<StdlibValue, StdlibExecError> {
        match self.implementation {
            FunctionImpl::Pure(function) => function(args),
            FunctionImpl::Capability {
                capability,
                operation,
            } => match host {
                Some(h) => h.call(capability, operation, args),
                None => Err(StdlibExecError::CapabilityRequired {
                    capability: capability.to_string(),
                    operation: operation.to_string(),
                }),
            },
        }
    }

    /// Execute this function without a capability host.
    ///
    /// For capability-backed functions this always returns `CapabilityRequired`.
    /// Use [`call_with_host`](Self::call_with_host) when a host is available.
    pub fn call(&self, args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
        self.call_with_host(args, None)
    }
}

// ── Entry table ───────────────────────────────────────────────────────────

/// Return all execution entries known to stdlib v1.
pub fn stdlib_function_entries() -> Vec<FunctionEntry> {
    vec![
        pure(
            "std.core.option.map",
            "std.core",
            "map",
            &["Option<T>", "Fn(T) -> U"],
            "Option<U>",
            option_map,
        ),
        pure(
            "std.core.option.and_then",
            "std.core",
            "and_then",
            &["Option<T>", "Fn(T) -> Option<U>"],
            "Option<U>",
            option_and_then,
        ),
        pure(
            "std.core.option.unwrap_or",
            "std.core",
            "unwrap_or",
            &["Option<T>", "T"],
            "T",
            option_unwrap_or,
        ),
        pure(
            "std.core.option.ok_or",
            "std.core",
            "ok_or",
            &["Option<T>", "E"],
            "Result<T, E>",
            option_ok_or,
        ),
        pure(
            "std.core.result.map",
            "std.core",
            "map",
            &["Result<T, E>", "Fn(T) -> U"],
            "Result<U, E>",
            result_map,
        ),
        pure(
            "std.core.result.and_then",
            "std.core",
            "and_then",
            &["Result<T, E>", "Fn(T) -> Result<U, E>"],
            "Result<U, E>",
            result_and_then,
        ),
        pure(
            "std.core.result.unwrap_or",
            "std.core",
            "unwrap_or",
            &["Result<T, E>", "T"],
            "T",
            result_unwrap_or,
        ),
        pure(
            "std.core.option.transpose",
            "std.core",
            "transpose",
            &["Option<Result<T, E>>"],
            "Result<Option<T>, E>",
            option_transpose,
        ),
        pure(
            "std.core.option.collect_results",
            "std.core",
            "collect_results",
            &["List<Result<T, E>>"],
            "Result<List<T>, E>",
            option_collect_results,
        ),
        pure(
            "std.core.result.transpose",
            "std.core",
            "transpose",
            &["Result<Option<T>, E>"],
            "Option<Result<T, E>>",
            result_transpose,
        ),
        pure(
            "std.collections.list.length",
            "std.collections",
            "length",
            &["List<T>"],
            "UInt",
            list_length,
        ),
        pure(
            "std.collections.list.is_empty",
            "std.collections",
            "is_empty",
            &["List<T>"],
            "Bool",
            list_is_empty,
        ),
        pure(
            "std.collections.list.push",
            "std.collections",
            "push",
            &["List<T>", "T"],
            "List<T>",
            list_push,
        ),
        pure(
            "std.collections.list.get",
            "std.collections",
            "get",
            &["List<T>", "UInt"],
            "Option<T>",
            list_get,
        ),
        pure(
            "std.collections.map.get",
            "std.collections",
            "get",
            &["Map<Text, V>", "Text"],
            "Option<V>",
            map_get,
        ),
        pure(
            "std.collections.map.contains_key",
            "std.collections",
            "contains_key",
            &["Map<Text, V>", "Text"],
            "Bool",
            map_contains_key,
        ),
        pure(
            "std.collections.map.length",
            "std.collections",
            "length",
            &["Map<Text, V>"],
            "UInt",
            map_length,
        ),
        pure(
            "std.collections.map.insert",
            "std.collections",
            "insert",
            &["Map<Text, V>", "Text", "V"],
            "Map<Text, V>",
            map_insert,
        ),
        pure(
            "std.collections.set.contains",
            "std.collections",
            "contains",
            &["List<T>", "T"],
            "Bool",
            set_contains,
        ),
        pure(
            "std.collections.set.length",
            "std.collections",
            "length",
            &["List<T>"],
            "UInt",
            set_length,
        ),
        pure(
            "std.collections.set.insert",
            "std.collections",
            "insert",
            &["List<T>", "T"],
            "List<T>",
            set_insert,
        ),
        pure(
            "std.text.trim",
            "std.text",
            "trim",
            &["Text"],
            "Text",
            text_trim,
        ),
        pure(
            "std.text.split",
            "std.text",
            "split",
            &["Text", "Text"],
            "List<Text>",
            text_split,
        ),
        pure(
            "std.text.join",
            "std.text",
            "join",
            &["List<Text>", "Text"],
            "Text",
            text_join,
        ),
        // "Text?" marks the second param as optional (no convention existed
        // before v1; `?` suffix signals variadic/optional for introspection).
        // Calling with 1 arg selects NFC; 2 args select the form ("nfc"|"nfd").
        pure(
            "std.text.normalize",
            "std.text",
            "normalize",
            &["Text", "Text?"],
            "Text",
            text_normalize,
        ),
        pure(
            "std.text.encode",
            "std.text",
            "encode",
            &["Text"],
            "Bytes",
            text_encode,
        ),
        pure(
            "std.text.decode",
            "std.text",
            "decode",
            &["Bytes"],
            "Result<Text, DecodeError>",
            text_decode,
        ),
        pure(
            "std.text.format",
            "std.text",
            "format",
            &["Text", "List<Text>"],
            "Text",
            text_format,
        ),
        pure(
            "std.text.regex",
            "std.text",
            "regex",
            &["Text", "Text"],
            "Bool",
            text_regex,
        ),
        pure(
            "std.crypto.hash",
            "std.crypto",
            "hash",
            &["Bytes"],
            "Bytes",
            crypto_hash,
        ),
        pure(
            "std.crypto.hmac",
            "std.crypto",
            "hmac",
            &["Bytes", "Bytes"],
            "Bytes",
            crypto_hmac,
        ),
        pure(
            "std.crypto.constant_time_eq",
            "std.crypto",
            "constant_time_eq",
            &["Bytes", "Bytes"],
            "Bool",
            crypto_constant_time_eq,
        ),
        pure(
            "std.encoding.base64_encode",
            "std.encoding",
            "base64_encode",
            &["Bytes"],
            "Text",
            encoding_base64_encode,
        ),
        pure(
            "std.encoding.base64_decode",
            "std.encoding",
            "base64_decode",
            &["Text"],
            "Result<Bytes, DecodeError>",
            encoding_base64_decode,
        ),
        pure(
            "std.encoding.hex_encode",
            "std.encoding",
            "hex_encode",
            &["Bytes"],
            "Text",
            encoding_hex_encode,
        ),
        pure(
            "std.encoding.hex_decode",
            "std.encoding",
            "hex_decode",
            &["Text"],
            "Result<Bytes, DecodeError>",
            encoding_hex_decode,
        ),
        pure(
            "std.json.parse",
            "std.json",
            "parse",
            &["Text"],
            "Result<Json, DecodeError>",
            json_parse,
        ),
        pure(
            "std.json.stringify",
            "std.json",
            "stringify",
            &["Map"],
            "Text",
            json_stringify,
        ),
        pure(
            "std.concurrent.channel_new",
            "std.concurrent",
            "channel_new",
            &["Int"],
            "Channel",
            concurrent_channel_new,
        ),
        pure(
            "std.concurrent.channel_send",
            "std.concurrent",
            "channel_send",
            &["Channel", "T"],
            "Result<Unit, Text>",
            concurrent_channel_send,
        ),
        pure(
            "std.concurrent.channel_recv",
            "std.concurrent",
            "channel_recv",
            &["Channel"],
            "Option<T>",
            concurrent_channel_recv,
        ),
        pure(
            "std.concurrent.channel_len",
            "std.concurrent",
            "channel_len",
            &["Channel"],
            "Int",
            concurrent_channel_len,
        ),
        pure(
            "std.text.length_graphemes",
            "std.text",
            "length_graphemes",
            &["Text"],
            "Int",
            text_length_graphemes_exec,
        ),
        pure(
            "std.text.starts_with",
            "std.text",
            "starts_with",
            &["Text", "Text"],
            "Bool",
            text_starts_with_exec,
        ),
        pure(
            "std.text.ends_with",
            "std.text",
            "ends_with",
            &["Text", "Text"],
            "Bool",
            text_ends_with_exec,
        ),
        pure(
            "std.text.contains",
            "std.text",
            "contains",
            &["Text", "Text"],
            "Bool",
            text_contains_exec,
        ),
        pure(
            "std.text.replace",
            "std.text",
            "replace",
            &["Text", "Text", "Text"],
            "Text",
            text_replace_exec,
        ),
        pure(
            "std.time.duration_since",
            "std.time",
            "duration_since",
            &["Int", "Int"],
            "Int",
            time_duration_since_exec,
        ),
        pure(
            "std.time.add_duration",
            "std.time",
            "add_duration",
            &["Int", "Int"],
            "Int",
            time_add_duration_exec,
        ),
        pure(
            "std.time.instant_to_ms",
            "std.time",
            "instant_to_ms",
            &["Int"],
            "Int",
            time_instant_to_ms_exec,
        ),
        pure(
            "std.collections.list.map",
            "std.collections",
            "map",
            &["List<T>", "Fn(T) -> U"],
            "List<U>",
            list_map_exec,
        ),
        pure(
            "std.collections.list.filter",
            "std.collections",
            "filter",
            &["List<T>", "Fn(T) -> Bool"],
            "List<T>",
            list_filter_exec,
        ),
        pure(
            "std.collections.list.fold",
            "std.collections",
            "fold",
            &["List<T>", "U", "Fn(List<[U, T]>) -> U"],
            "U",
            list_fold_exec,
        ),
        pure(
            "std.collections.list.concat",
            "std.collections",
            "concat",
            &["List<T>", "List<T>"],
            "List<T>",
            list_concat_exec,
        ),
        pure(
            "std.iter.map",
            "std.iter",
            "map",
            &["List<T>", "Fn(T) -> U"],
            "List<U>",
            iter_map_exec,
        ),
        pure(
            "std.iter.filter",
            "std.iter",
            "filter",
            &["List<T>", "Fn(T) -> Bool"],
            "List<T>",
            iter_filter_exec,
        ),
        pure(
            "std.iter.any",
            "std.iter",
            "any",
            &["List<T>", "Fn(T) -> Bool"],
            "Bool",
            iter_any_exec,
        ),
        pure(
            "std.iter.all",
            "std.iter",
            "all",
            &["List<T>", "Fn(T) -> Bool"],
            "Bool",
            iter_all_exec,
        ),
        pure(
            "std.iter.find",
            "std.iter",
            "find",
            &["List<T>", "Fn(T) -> Bool"],
            "Option<T>",
            iter_find_exec,
        ),
        pure(
            "std.iter.position",
            "std.iter",
            "position",
            &["List<T>", "Fn(T) -> Bool"],
            "Option<Int>",
            iter_position_exec,
        ),
        pure(
            "std.iter.fold",
            "std.iter",
            "fold",
            &["List<T>", "U", "Fn(List<[U, T]>) -> U"],
            "U",
            iter_fold_exec,
        ),
        pure(
            "std.iter.traverse",
            "std.iter",
            "traverse",
            &["List<T>", "Fn(T) -> Result<U, E>"],
            "Result<List<U>, E>",
            iter_traverse_exec,
        ),
        pure(
            "std.numeric.checked_add",
            "std.numeric",
            "checked_add",
            &["Int", "Int"],
            "Option<Int>",
            numeric_checked_add,
        ),
        pure(
            "std.numeric.checked_sub",
            "std.numeric",
            "checked_sub",
            &["Int", "Int"],
            "Option<Int>",
            numeric_checked_sub,
        ),
        pure(
            "std.numeric.checked_mul",
            "std.numeric",
            "checked_mul",
            &["Int", "Int"],
            "Option<Int>",
            numeric_checked_mul,
        ),
        pure(
            "std.numeric.wrapping_add",
            "std.numeric",
            "wrapping_add",
            &["Int", "Int"],
            "Int",
            numeric_wrapping_add,
        ),
        pure(
            "std.numeric.saturating_add",
            "std.numeric",
            "saturating_add",
            &["Int", "Int"],
            "Int",
            numeric_saturating_add,
        ),
        pure(
            "std.numeric.narrow_to_i32",
            "std.numeric",
            "narrow_to_i32",
            &["Int"],
            "Result<Int32, ArithError>",
            numeric_narrow_to_i32,
        ),
        pure(
            "std.numeric.narrow_to_u32",
            "std.numeric",
            "narrow_to_u32",
            &["Int"],
            "Result<UInt32, ArithError>",
            numeric_narrow_to_u32,
        ),
        pure(
            "std.bytes.length",
            "std.bytes",
            "length",
            &["Bytes"],
            "Int",
            bytes_length,
        ),
        pure(
            "std.bytes.at",
            "std.bytes",
            "at",
            &["Bytes", "Int"],
            "Option<Int>",
            bytes_at,
        ),
        pure(
            "std.bytes.slice",
            "std.bytes",
            "slice",
            &["Bytes", "Int", "Int"],
            "Option<Bytes>",
            bytes_slice,
        ),
        pure(
            "std.bytes.concat",
            "std.bytes",
            "concat",
            &["Bytes", "Bytes"],
            "Bytes",
            bytes_concat,
        ),
        pure(
            "std.bytes.empty",
            "std.bytes",
            "empty",
            &["Bytes"],
            "Bool",
            bytes_empty,
        ),
        capability(
            "std.time.now",
            "std.time",
            "now",
            &[],
            "Instant",
            "clock.now",
            "now",
        ),
        capability(
            "std.random.next_int",
            "std.random",
            "next_int",
            &[],
            "Int",
            "random.int",
            "next_int",
        ),
        capability(
            "std.random.next_float",
            "std.random",
            "next_float",
            &[],
            "Float",
            "random.float",
            "next_float",
        ),
        capability(
            "std.io.read",
            "std.io",
            "read",
            &["Handle"],
            "Bytes",
            "io.stdin",
            "read",
        ),
        capability(
            "std.io.write",
            "std.io",
            "write",
            &["Handle", "Bytes"],
            "UInt",
            "io.stdout",
            "write",
        ),
        capability(
            "std.io.flush",
            "std.io",
            "flush",
            &["Handle"],
            "Unit",
            "io.stdout",
            "flush",
        ),
        capability(
            "std.io.seek",
            "std.io",
            "seek",
            &["Handle", "UInt"],
            "Unit",
            "io.seek",
            "seek",
        ),
        capability(
            "std.fs.open",
            "std.fs",
            "open",
            &["Path"],
            "FileHandle",
            "file.read",
            "open",
        ),
        capability(
            "std.fs.read",
            "std.fs",
            "read",
            &["Path"],
            "Bytes",
            "file.read",
            "read",
        ),
        // Convenience alias: read entire file contents as Bytes.
        capability(
            "std.fs.read_file",
            "std.fs",
            "read_file",
            &["Path"],
            "Bytes",
            "file.read",
            "read",
        ),
        capability(
            "std.fs.write",
            "std.fs",
            "write",
            &["Path", "Bytes"],
            "Unit",
            "file.write",
            "write",
        ),
        capability(
            "std.fs.delete",
            "std.fs",
            "delete",
            &["Path"],
            "Unit",
            "file.delete",
            "delete",
        ),
        capability(
            "std.fs.list",
            "std.fs",
            "list",
            &["Path"],
            "List<Path>",
            "file.list",
            "list",
        ),
        capability(
            "std.fs.stat",
            "std.fs",
            "stat",
            &["Path"],
            "FileMetadata",
            "file.read",
            "stat",
        ),
        capability(
            "std.net.connect",
            "std.net",
            "connect",
            &["Url"],
            "Connection",
            "network.connect",
            "connect",
        ),
        capability(
            "std.net.listen",
            "std.net",
            "listen",
            &["Url"],
            "Listener",
            "network.bind",
            "listen",
        ),
        capability(
            "std.net.send",
            "std.net",
            "send",
            &["Connection", "Bytes"],
            "UInt",
            "network.connect",
            "send",
        ),
        capability(
            "std.net.receive",
            "std.net",
            "receive",
            &["Connection"],
            "Bytes",
            "network.connect",
            "receive",
        ),
        capability(
            "std.http.request",
            "std.http",
            "request",
            &["HttpRequest"],
            "HttpResponse",
            "http.call",
            "request",
        ),
        capability(
            "std.http.serve",
            "std.http",
            "serve",
            &["HttpHandler"],
            "Server",
            "http.serve",
            "serve",
        ),
        capability(
            "std.process.spawn",
            "std.process",
            "spawn",
            &["Command"],
            "ProcessHandle",
            "process.spawn",
            "spawn",
        ),
        capability(
            "std.process.wait",
            "std.process",
            "wait",
            &["ProcessHandle"],
            "ExitCode",
            "process.wait",
            "wait",
        ),
        capability(
            "std.process.kill",
            "std.process",
            "kill",
            &["ProcessHandle"],
            "Unit",
            "process.signal",
            "kill",
        ),
        capability(
            "std.env.get",
            "std.env",
            "get",
            &["Text"],
            "Option<Text>",
            "env.read",
            "get",
        ),
        capability(
            "std.env.set",
            "std.env",
            "set",
            &["Text", "Text"],
            "Unit",
            "env.write",
            "set",
        ),
        capability(
            "std.env.list",
            "std.env",
            "list",
            &[],
            "Map<Text, Text>",
            "env.read",
            "list",
        ),
        capability(
            "std.log.log",
            "std.log",
            "log",
            &["LogLevel", "Text"],
            "Unit",
            "log.write",
            "log",
        ),
        capability(
            "std.trace.span",
            "std.trace",
            "span",
            &["Text"],
            "Span",
            "trace.emit",
            "span",
        ),
        capability(
            "std.trace.event",
            "std.trace",
            "event",
            &["Text"],
            "Unit",
            "trace.emit",
            "event",
        ),
    ]
}

// ── Public dispatch API ───────────────────────────────────────────────────

pub fn find_function_entry(id: &str) -> Option<FunctionEntry> {
    stdlib_function_entries()
        .into_iter()
        .find(|entry| entry.id == id)
}

pub fn call_pure_stdlib(id: &str, args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    find_function_entry(id)
        .ok_or_else(|| StdlibExecError::UnknownFunction(id.to_string()))?
        .call(args)
}

/// Execute a stdlib function by ID, routing capability calls through `host`.
///
/// For pure functions the host is ignored.  For capability-backed functions
/// the call is dispatched to `host.call(capability, operation, args)`.
///
/// Returns [`StdlibExecError::UnknownFunction`] when `id` is not registered.
pub fn call_effectful_stdlib(
    id: &str,
    args: &[StdlibValue],
    host: &dyn StdlibCapabilityDispatch,
) -> Result<StdlibValue, StdlibExecError> {
    find_function_entry(id)
        .ok_or_else(|| StdlibExecError::UnknownFunction(id.to_string()))?
        .call_with_host(args, Some(host))
}

// ── Entry constructors (pub(super) for tests in exec.rs) ──────────────────

pub(super) fn pure(
    id: &'static str,
    module: &'static str,
    name: &'static str,
    params: &'static [&'static str],
    return_type: &'static str,
    function: PureStdlibFn,
) -> FunctionEntry {
    FunctionEntry {
        id,
        module,
        name,
        params,
        return_type,
        implementation: FunctionImpl::Pure(function),
    }
}

pub(super) fn capability(
    id: &'static str,
    module: &'static str,
    name: &'static str,
    params: &'static [&'static str],
    return_type: &'static str,
    capability: &'static str,
    operation: &'static str,
) -> FunctionEntry {
    FunctionEntry {
        id,
        module,
        name,
        params,
        return_type,
        implementation: FunctionImpl::Capability {
            capability,
            operation,
        },
    }
}
