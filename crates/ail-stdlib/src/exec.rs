// ── ail-stdlib::exec ──────────────────────────────────────────────────────
//
// Executable stdlib function registry.
//
// The metadata registry in `v1` describes the public API shape. This module
// provides the execution-facing table: pure functions carry Rust function
// pointers, while effectful functions carry a capability + operation pair for
// runtime handler dispatch.

use std::collections::BTreeMap;

use crate::{crypto, text};

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
    pub fn call(&self, args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
        match self.implementation {
            FunctionImpl::Pure(function) => function(args),
            FunctionImpl::Capability {
                capability,
                operation,
            } => Err(StdlibExecError::CapabilityRequired {
                capability: capability.to_string(),
                operation: operation.to_string(),
            }),
        }
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
