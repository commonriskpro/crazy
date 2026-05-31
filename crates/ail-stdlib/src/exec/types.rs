// ── ail-stdlib::exec::types ───────────────────────────────────────────────
//
// Core value and error types for the executable stdlib dispatch layer.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::concurrent;

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
    Tuple(Vec<StdlibValue>),
    Map(BTreeMap<String, StdlibValue>),
    Option(Option<Box<StdlibValue>>),
    Result(Result<Box<StdlibValue>, Box<StdlibValue>>),
    Function(fn(StdlibValue) -> Result<StdlibValue, StdlibExecError>),
    /// A bounded channel handle. Arc ensures shared ownership across clones.
    /// PartialEq is always false (reference identity semantics, like Function).
    Channel(Arc<Mutex<concurrent::Channel<StdlibValue>>>),
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
            (Self::Tuple(a), Self::Tuple(b)) => a == b,
            (Self::Map(a), Self::Map(b)) => a == b,
            (Self::Option(a), Self::Option(b)) => a == b,
            (Self::Result(a), Self::Result(b)) => a == b,
            (Self::Function(_), Self::Function(_)) => false,
            (Self::Channel(_), Self::Channel(_)) => false,
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
