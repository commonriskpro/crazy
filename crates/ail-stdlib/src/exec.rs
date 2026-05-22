// ── ail-stdlib::exec ──────────────────────────────────────────────────────
//
// Executable stdlib function registry.
//
// The metadata registry in `v1` describes the public API shape. This module
// provides the execution-facing table: pure functions carry Rust function
// pointers, while effectful functions carry a capability + operation pair for
// runtime handler dispatch.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::{crypto, encoding, json, text};

/// Runtime value shape understood by the stdlib executable dispatch layer.
#[derive(Clone, Debug)]
pub enum StdlibValue {
    Unit,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    List(Vec<StdlibValue>),
    Map(BTreeMap<String, StdlibValue>),
    Option(Option<Box<StdlibValue>>),
    Result(Result<Box<StdlibValue>, Box<StdlibValue>>),
    Function(fn(StdlibValue) -> Result<StdlibValue, StdlibExecError>),
}

impl PartialEq for StdlibValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Unit, Self::Unit) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Map(a), Self::Map(b)) => a == b,
            (Self::Option(a), Self::Option(b)) => a == b,
            (Self::Result(a), Self::Result(b)) => a == b,
            (Self::Function(_), Self::Function(_)) => false,
            _ => false,
        }
    }
}

impl Eq for StdlibValue {}

/// Error returned by executable stdlib functions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StdlibExecError {
    UnknownFunction(String),
    CapabilityRequired {
        capability: String,
        operation: String,
    },
    Arity {
        expected: usize,
        actual: usize,
    },
    Type {
        expected: &'static str,
    },
    Message(String),
}

impl std::fmt::Display for StdlibExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StdlibExecError::UnknownFunction(id) => write!(f, "unknown stdlib function: {id}"),
            StdlibExecError::CapabilityRequired {
                capability,
                operation,
            } => {
                write!(f, "capability required: {capability}.{operation}")
            }
            StdlibExecError::Arity { expected, actual } => {
                write!(f, "expected {expected} arguments, got {actual}")
            }
            StdlibExecError::Type { expected } => write!(f, "expected {expected}"),
            StdlibExecError::Message(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for StdlibExecError {}

pub type PureStdlibFn = fn(&[StdlibValue]) -> Result<StdlibValue, StdlibExecError>;

// ── StdlibCapabilityDispatch ──────────────────────────────────────────────

/// Trait for dispatching stdlib capability calls to a host implementation.
///
/// Implementors provide in-memory or runtime-backed services for effectful
/// stdlib operations (clock, env, io, fs, log, trace, random).
pub trait StdlibCapabilityDispatch {
    fn call(
        &self,
        capability: &str,
        operation: &str,
        args: &[StdlibValue],
    ) -> Result<StdlibValue, StdlibExecError>;
}

// ── InMemoryCapabilityHost ────────────────────────────────────────────────

/// Deterministic in-memory host for testing effectful stdlib functions.
///
/// Builder pattern:
/// ```
/// # use ail_stdlib::exec::InMemoryCapabilityHost;
/// let host = InMemoryCapabilityHost::new()
///     .with_env("PORT", "8080")
///     .with_fixed_clock(0);
/// ```
pub struct InMemoryCapabilityHost {
    env: BTreeMap<String, String>,
    files: BTreeMap<String, Vec<u8>>,
    stdout: RefCell<Vec<u8>>,
    logs: RefCell<Vec<(String, String)>>,
    fixed_clock: Option<i64>,
    rng_seed: u64,
}

impl InMemoryCapabilityHost {
    pub fn new() -> Self {
        Self {
            env: BTreeMap::new(),
            files: BTreeMap::new(),
            stdout: RefCell::new(Vec::new()),
            logs: RefCell::new(Vec::new()),
            fixed_clock: None,
            rng_seed: 0,
        }
    }

    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_file(mut self, path: &str, content: &[u8]) -> Self {
        self.files.insert(path.to_string(), content.to_vec());
        self
    }

    pub fn with_fixed_clock(mut self, epoch_ms: i64) -> Self {
        self.fixed_clock = Some(epoch_ms);
        self
    }

    pub fn with_rng_seed(mut self, seed: u64) -> Self {
        self.rng_seed = seed;
        self
    }

    /// Retrieve captured stdout bytes.
    pub fn stdout_bytes(&self) -> Vec<u8> {
        self.stdout.borrow().clone()
    }

    /// Retrieve captured log entries.
    pub fn log_entries(&self) -> Vec<(String, String)> {
        self.logs.borrow().clone()
    }
}

impl Default for InMemoryCapabilityHost {
    fn default() -> Self {
        Self::new()
    }
}

impl StdlibCapabilityDispatch for InMemoryCapabilityHost {
    fn call(
        &self,
        capability: &str,
        operation: &str,
        args: &[StdlibValue],
    ) -> Result<StdlibValue, StdlibExecError> {
        match (capability, operation) {
            // ── clock ─────────────────────────────────────────────────────
            ("clock.now", "now") => Ok(StdlibValue::Int(
                self.fixed_clock.unwrap_or(0),
            )),

            // ── env.read ──────────────────────────────────────────────────
            ("env.read", "get") => {
                let StdlibValue::Text(key) = args.first().ok_or(StdlibExecError::Arity {
                    expected: 1,
                    actual: 0,
                })? else {
                    return Err(StdlibExecError::Type { expected: "Text" });
                };
                Ok(StdlibValue::Option(
                    self.env
                        .get(key)
                        .cloned()
                        .map(|v| Box::new(StdlibValue::Text(v))),
                ))
            }
            ("env.read", "list") => {
                let map = self
                    .env
                    .iter()
                    .map(|(k, v)| (k.clone(), StdlibValue::Text(v.clone())))
                    .collect();
                Ok(StdlibValue::Map(map))
            }

            // ── env.write ─────────────────────────────────────────────────
            // env.write.set is a no-op in InMemoryCapabilityHost (cannot set OS env)
            ("env.write", "set") => Ok(StdlibValue::Unit),

            // ── io.stdin ──────────────────────────────────────────────────
            ("io.stdin", "read") => Ok(StdlibValue::Bytes(vec![])),

            // ── io.stdout ─────────────────────────────────────────────────
            ("io.stdout", "write") => {
                let StdlibValue::Bytes(bytes) = args.first().ok_or(StdlibExecError::Arity {
                    expected: 1,
                    actual: 0,
                })? else {
                    return Err(StdlibExecError::Type { expected: "Bytes" });
                };
                let len = bytes.len() as i64;
                self.stdout.borrow_mut().extend_from_slice(bytes);
                Ok(StdlibValue::Int(len))
            }
            ("io.stdout", "flush") => Ok(StdlibValue::Unit),

            // ── file.read ─────────────────────────────────────────────────
            ("file.read", "read") | ("file.read", "open") | ("file.read", "stat") => {
                let StdlibValue::Text(path) = args.first().ok_or(StdlibExecError::Arity {
                    expected: 1,
                    actual: 0,
                })? else {
                    return Err(StdlibExecError::Type { expected: "Text" });
                };
                match (operation, self.files.get(path)) {
                    ("read", Some(content)) => Ok(StdlibValue::Bytes(content.clone())),
                    ("open", _) => Ok(StdlibValue::Unit),
                    ("stat", _) => Ok(StdlibValue::Unit),
                    _ => Err(StdlibExecError::Message(format!("file not found: {path}"))),
                }
            }
            ("file.write", "write") => Ok(StdlibValue::Unit),
            ("file.delete", "delete") => Ok(StdlibValue::Unit),
            ("file.list", "list") => Ok(StdlibValue::List(vec![])),

            // ── log.write ─────────────────────────────────────────────────
            ("log.write", "log") => {
                if let (Some(StdlibValue::Text(level)), Some(StdlibValue::Text(msg))) =
                    (args.first(), args.get(1))
                {
                    self.logs.borrow_mut().push((level.clone(), msg.clone()));
                }
                Ok(StdlibValue::Unit)
            }

            // ── trace.emit ────────────────────────────────────────────────
            ("trace.emit", "span") => Ok(StdlibValue::Text("span-0".to_string())),
            ("trace.emit", "event") => Ok(StdlibValue::Unit),

            // ── random ────────────────────────────────────────────────────
            ("random.int", "next_int") => Ok(StdlibValue::Int(self.rng_seed as i64)),
            ("random.float", "next_float") => Ok(StdlibValue::Float(self.rng_seed as f64 / 1000.0)),

            _ => Err(StdlibExecError::CapabilityRequired {
                capability: capability.to_string(),
                operation: operation.to_string(),
            }),
        }
    }
}

/// Executable implementation behind a stdlib function entry.
#[derive(Clone, Copy)]
pub enum FunctionImpl {
    Pure(PureStdlibFn),
    Capability {
        capability: &'static str,
        operation: &'static str,
    },
}

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
            "std.collections.list.length",
            "std.collections",
            "length",
            &["List<T>"],
            "UInt",
            list_length,
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
        pure(
            "std.text.normalize",
            "std.text",
            "normalize",
            &["Text"],
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

fn pure(
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

fn capability(
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

fn expect_arity(args: &[StdlibValue], expected: usize) -> Result<(), StdlibExecError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(StdlibExecError::Arity {
            expected,
            actual: args.len(),
        })
    }
}

fn option_map(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Option(option) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Option" });
    };
    let StdlibValue::Function(function) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    option
        .clone()
        .map(|value| function(*value).map(|mapped| StdlibValue::Option(Some(Box::new(mapped)))))
        .unwrap_or(Ok(StdlibValue::Option(None)))
}

fn option_and_then(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Option(option) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Option" });
    };
    let StdlibValue::Function(function) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    match option.clone() {
        Some(value) => match function(*value)? {
            StdlibValue::Option(next) => Ok(StdlibValue::Option(next)),
            _ => Err(StdlibExecError::Type { expected: "Option" }),
        },
        None => Ok(StdlibValue::Option(None)),
    }
}

fn option_unwrap_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Option(option) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Option" });
    };
    Ok(option
        .clone()
        .map(|value| *value)
        .unwrap_or_else(|| args[1].clone()))
}

fn option_ok_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Option(option) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Option" });
    };
    Ok(match option.clone() {
        Some(value) => StdlibValue::Result(Ok(value)),
        None => StdlibValue::Result(Err(Box::new(args[1].clone()))),
    })
}

fn result_map(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Result(result) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Result" });
    };
    let StdlibValue::Function(function) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    Ok(match result.clone() {
        Ok(value) => StdlibValue::Result(Ok(Box::new(function(*value)?))),
        Err(error) => StdlibValue::Result(Err(error)),
    })
}

fn result_and_then(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Result(result) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Result" });
    };
    let StdlibValue::Function(function) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    match result.clone() {
        Ok(value) => match function(*value)? {
            StdlibValue::Result(next) => Ok(StdlibValue::Result(next)),
            _ => Err(StdlibExecError::Type { expected: "Result" }),
        },
        Err(error) => Ok(StdlibValue::Result(Err(error))),
    }
}

fn result_unwrap_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Result(result) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Result" });
    };
    Ok(result
        .clone()
        .map(|value| *value)
        .unwrap_or_else(|_| args[1].clone()))
}

fn list_length(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::List(items) => Ok(StdlibValue::Int(items.len() as i64)),
        _ => Err(StdlibExecError::Type { expected: "List" }),
    }
}

fn list_push(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(mut items) = args[0].clone() else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    items.push(args[1].clone());
    Ok(StdlibValue::List(items))
}

fn list_get(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let StdlibValue::Int(index) = args[1] else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    let value = usize::try_from(index)
        .ok()
        .and_then(|index| items.get(index).cloned())
        .map(Box::new);
    Ok(StdlibValue::Option(value))
}

fn map_get(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Map(map) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Map" });
    };
    let StdlibValue::Text(key) = &args[1] else {
        return Err(StdlibExecError::Type { expected: "Text" });
    };
    Ok(StdlibValue::Option(map.get(key).cloned().map(Box::new)))
}

fn map_insert(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let StdlibValue::Map(mut map) = args[0].clone() else {
        return Err(StdlibExecError::Type { expected: "Map" });
    };
    let StdlibValue::Text(key) = &args[1] else {
        return Err(StdlibExecError::Type { expected: "Text" });
    };
    map.insert(key.clone(), args[2].clone());
    Ok(StdlibValue::Map(map))
}

fn set_contains(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    Ok(StdlibValue::Bool(items.contains(&args[1])))
}

fn set_insert(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(mut items) = args[0].clone() else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    if !items.contains(&args[1]) {
        items.push(args[1].clone());
    }
    Ok(StdlibValue::List(items))
}

fn text_trim(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(value) => Ok(StdlibValue::Text(text::text_trim(value))),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

fn text_split(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Text(value), StdlibValue::Text(delimiter)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Text, Text",
        });
    };
    Ok(StdlibValue::List(
        text::text_split(value, delimiter)
            .into_iter()
            .map(StdlibValue::Text)
            .collect(),
    ))
}

fn text_join(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::List(parts), StdlibValue::Text(separator)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "List<Text>, Text",
        });
    };
    let strings = parts
        .iter()
        .map(|part| match part {
            StdlibValue::Text(value) => Ok(value.as_str()),
            _ => Err(StdlibExecError::Type { expected: "Text" }),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(StdlibValue::Text(text::text_join(&strings, separator)))
}

fn text_normalize(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(value) => Ok(StdlibValue::Text(text::text_normalize(
            value,
            text::NormalizeForm::Nfc,
        ))),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

fn text_encode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(value) => Ok(StdlibValue::Bytes(text::text_to_bytes(value))),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

fn text_decode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Bytes(bytes) => Ok(match text::text_from_bytes(bytes) {
            Ok(value) => StdlibValue::Result(Ok(Box::new(StdlibValue::Text(value)))),
            Err(error) => StdlibValue::Result(Err(Box::new(StdlibValue::Text(error)))),
        }),
        _ => Err(StdlibExecError::Type { expected: "Bytes" }),
    }
}

fn text_format(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Text(template), StdlibValue::List(values)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Text, List<Text>",
        });
    };
    let mut output = template.clone();
    for value in values {
        let StdlibValue::Text(value) = value else {
            return Err(StdlibExecError::Type { expected: "Text" });
        };
        output = output.replacen("{}", value, 1);
    }
    Ok(StdlibValue::Text(output))
}

fn text_regex(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Text(value), StdlibValue::Text(pattern)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Text, Text",
        });
    };
    Ok(StdlibValue::Bool(value.contains(pattern)))
}

fn crypto_hash(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Bytes(bytes) => Ok(StdlibValue::Bytes(crypto::Hash::blake3(bytes).0.to_vec())),
        _ => Err(StdlibExecError::Type { expected: "Bytes" }),
    }
}

fn crypto_hmac(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Bytes(key), StdlibValue::Bytes(msg)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Bytes, Bytes" });
    };
    Ok(StdlibValue::Bytes(
        crypto::Hmac::compute(key, msg).0.to_vec(),
    ))
}

fn crypto_constant_time_eq(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Bytes(a), StdlibValue::Bytes(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Bytes, Bytes" });
    };
    Ok(StdlibValue::Bool(crypto::constant_time_eq(a, b)))
}

fn encoding_base64_encode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Bytes(bytes) => Ok(StdlibValue::Text(encoding::base64_encode(bytes))),
        _ => Err(StdlibExecError::Type { expected: "Bytes" }),
    }
}

fn encoding_base64_decode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(s) => Ok(StdlibValue::Result(
            encoding::base64_decode(s)
                .map(|bytes| Box::new(StdlibValue::Bytes(bytes)))
                .map_err(|e| Box::new(StdlibValue::Text(e.0))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

fn encoding_hex_encode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Bytes(bytes) => Ok(StdlibValue::Text(encoding::hex_encode(bytes))),
        _ => Err(StdlibExecError::Type { expected: "Bytes" }),
    }
}

fn encoding_hex_decode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(s) => Ok(StdlibValue::Result(
            encoding::hex_decode(s)
                .map(|bytes| Box::new(StdlibValue::Bytes(bytes)))
                .map_err(|e| Box::new(StdlibValue::Text(e.0))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

/// Convert a `json::Json` value into a `StdlibValue`.
fn json_to_stdlib(v: json::Json) -> StdlibValue {
    match v {
        json::Json::Null => StdlibValue::Unit,
        json::Json::Bool(b) => StdlibValue::Bool(b),
        json::Json::Number(n) => {
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                StdlibValue::Int(n as i64)
            } else {
                StdlibValue::Float(n)
            }
        }
        json::Json::Str(s) => StdlibValue::Text(s),
        json::Json::Array(arr) => {
            StdlibValue::List(arr.into_iter().map(json_to_stdlib).collect())
        }
        json::Json::Object(map) => StdlibValue::Map(
            map.into_iter()
                .map(|(k, v)| (k, json_to_stdlib(v)))
                .collect(),
        ),
    }
}

/// Convert a `StdlibValue` into a `json::Json` for stringification.
fn stdlib_to_json(v: &StdlibValue) -> json::Json {
    match v {
        StdlibValue::Unit => json::Json::Null,
        StdlibValue::Bool(b) => json::Json::Bool(*b),
        StdlibValue::Int(n) => json::Json::Number(*n as f64),
        StdlibValue::Float(f) => json::Json::Number(*f),
        StdlibValue::Text(s) => json::Json::Str(s.clone()),
        StdlibValue::Bytes(b) => json::Json::Str(encoding::hex_encode(b)),
        StdlibValue::List(items) => {
            json::Json::Array(items.iter().map(stdlib_to_json).collect())
        }
        StdlibValue::Map(map) => json::Json::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), stdlib_to_json(v)))
                .collect(),
        ),
        StdlibValue::Option(None) => json::Json::Null,
        StdlibValue::Option(Some(v)) => stdlib_to_json(v),
        StdlibValue::Result(Ok(v)) => stdlib_to_json(v),
        StdlibValue::Result(Err(e)) => stdlib_to_json(e),
        StdlibValue::Function(_) => json::Json::Null,
    }
}

fn json_parse(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(s) => Ok(StdlibValue::Result(
            json::parse(s)
                .map(|v| Box::new(json_to_stdlib(v)))
                .map_err(|e| Box::new(StdlibValue::Text(e.0))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

fn json_stringify(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    Ok(StdlibValue::Text(json::stringify(&stdlib_to_json(&args[0]))))
}

fn numeric_narrow_to_i32(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match args[0] {
        StdlibValue::Int(n) => Ok(StdlibValue::Result(
            i32::try_from(n)
                .map(|v| Box::new(StdlibValue::Int(v as i64)))
                .map_err(|e| Box::new(StdlibValue::Text(e.to_string()))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

fn numeric_narrow_to_u32(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match args[0] {
        StdlibValue::Int(n) => Ok(StdlibValue::Result(
            u32::try_from(n)
                .map(|v| Box::new(StdlibValue::Int(v as i64)))
                .map_err(|e| Box::new(StdlibValue::Text(e.to_string()))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        let result = host.call(
            "io.stdout",
            "write",
            &[StdlibValue::Bytes(vec![1u8, 2, 3])],
        );
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
        let result = call_pure_stdlib(
            "std.text.trim",
            &[StdlibValue::Text("  hi  ".to_string())],
        );
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
        assert_eq!(
            result,
            Ok(StdlibValue::Text("aGVsbG8=".to_string()))
        );
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
            Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Bytes(
                vec![0xca, 0xfe]
            )))))
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

    // Spec STDLIB-EXEC-1: std.numeric.narrow_to_i32 — ok
    #[test]
    fn exec_numeric_narrow_to_i32_ok() {
        let result = call_pure_stdlib(
            "std.numeric.narrow_to_i32",
            &[StdlibValue::Int(42)],
        );
        assert_eq!(
            result,
            Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Int(42)))))
        );
    }

    // Spec STDLIB-EXEC-1: std.numeric.narrow_to_i32 — overflow
    #[test]
    fn exec_numeric_narrow_to_i32_overflow() {
        let result = call_pure_stdlib(
            "std.numeric.narrow_to_i32",
            &[StdlibValue::Int(i64::MAX)],
        );
        assert!(
            matches!(result, Ok(StdlibValue::Result(Err(_)))),
            "overflow must return Err"
        );
    }

    // Spec STDLIB-EXEC-1: std.numeric.narrow_to_u32 — ok
    #[test]
    fn exec_numeric_narrow_to_u32_ok() {
        let result = call_pure_stdlib(
            "std.numeric.narrow_to_u32",
            &[StdlibValue::Int(100)],
        );
        assert_eq!(
            result,
            Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Int(100)))))
        );
    }

    // Spec STDLIB-EXEC-1: std.numeric.narrow_to_u32 — overflow (negative)
    #[test]
    fn exec_numeric_narrow_to_u32_negative() {
        let result = call_pure_stdlib(
            "std.numeric.narrow_to_u32",
            &[StdlibValue::Int(-1)],
        );
        assert!(
            matches!(result, Ok(StdlibValue::Result(Err(_)))),
            "negative value must return Err for u32"
        );
    }
}
