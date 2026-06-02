// ── ail-stdlib::exec::capability ─────────────────────────────────────────
//
// Capability dispatch trait and deterministic in-memory host for testing.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::{StdlibExecError, StdlibValue};

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
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    stdout: RefCell<Vec<u8>>,
    logs: RefCell<Vec<(String, String)>>,
    fixed_clock: Option<i64>,
    monotonic_clock: Option<i64>,
    rng_seed: u64,
}

impl InMemoryCapabilityHost {
    pub fn new() -> Self {
        Self {
            env: BTreeMap::new(),
            files: RefCell::new(BTreeMap::new()),
            stdout: RefCell::new(Vec::new()),
            logs: RefCell::new(Vec::new()),
            fixed_clock: None,
            monotonic_clock: None,
            rng_seed: 0,
        }
    }

    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_file(mut self, path: &str, content: &[u8]) -> Self {
        self.files
            .get_mut()
            .insert(path.to_string(), content.to_vec());
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

    /// Set a fixed return value for `clock.monotonic/now` (epoch_ms).
    pub fn with_monotonic(mut self, epoch_ms: i64) -> Self {
        self.monotonic_clock = Some(epoch_ms);
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
            ("clock.now", "now") => Ok(StdlibValue::Int(self.fixed_clock.unwrap_or(0))),

            // ── env.read ──────────────────────────────────────────────────
            ("env.read", "get") => {
                let StdlibValue::Text(key) = args.first().ok_or(StdlibExecError::Arity {
                    expected: 1,
                    actual: 0,
                })?
                else {
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
                })?
                else {
                    return Err(StdlibExecError::Type { expected: "Bytes" });
                };
                let len = bytes.len() as i64;
                self.stdout.borrow_mut().extend_from_slice(bytes);
                Ok(StdlibValue::Int(len))
            }
            ("io.stdout", "flush") => Ok(StdlibValue::Unit),

            // ── file.read ─────────────────────────────────────────────────
            ("file.read", "read") | ("file.read", "open") | ("file.read", "stat") => {
                let path = path_arg(args.first())?;
                let files = self.files.borrow();
                match (operation, files.get(path)) {
                    ("read", Some(content)) => Ok(StdlibValue::Bytes(content.clone())),
                    ("open", _) => Ok(StdlibValue::Unit),
                    ("stat", _) => Ok(StdlibValue::Unit),
                    _ => Err(StdlibExecError::Message(format!("file not found: {path}"))),
                }
            }
            ("file.write", "write") => {
                let path = path_arg(args.first())?.to_string();
                let StdlibValue::Bytes(bytes) = args.get(1).ok_or(StdlibExecError::Arity {
                    expected: 2,
                    actual: args.len(),
                })?
                else {
                    return Err(StdlibExecError::Type { expected: "Bytes" });
                };
                self.files.borrow_mut().insert(path, bytes.clone());
                Ok(StdlibValue::Unit)
            }
            ("file.delete", "delete") => {
                let path = path_arg(args.first())?;
                self.files.borrow_mut().remove(path);
                Ok(StdlibValue::Unit)
            }
            ("file.list", "list") => {
                let path = path_arg(args.first())?;
                let prefix = if path.ends_with('/') {
                    path.to_string()
                } else {
                    format!("{path}/")
                };
                let paths = self
                    .files
                    .borrow()
                    .keys()
                    .filter(|candidate| path.is_empty() || candidate.starts_with(&prefix))
                    .cloned()
                    .map(StdlibValue::Path)
                    .collect();
                Ok(StdlibValue::List(paths))
            }

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

            // ── clock.monotonic ───────────────────────────────────────────
            ("clock.monotonic", "now") => Ok(StdlibValue::Int(self.monotonic_clock.unwrap_or(0))),

            // ── random ────────────────────────────────────────────────────
            ("random.int", "next_int") => Ok(StdlibValue::Int(self.rng_seed as i64)),
            ("random.float", "next_float") => Ok(StdlibValue::Float(self.rng_seed as f64 / 1000.0)),
            ("random.bytes", "generate") => {
                let StdlibValue::Int(n) = args.first().ok_or(StdlibExecError::Arity {
                    expected: 1,
                    actual: 0,
                })?
                else {
                    return Err(StdlibExecError::Type { expected: "Int" });
                };
                let count = (*n).max(0) as usize;
                let mut rng =
                    crate::random::DeterministicRng::new(crate::random::Seed::new(self.rng_seed));
                Ok(StdlibValue::Bytes(rng.random_bytes(count)))
            }

            // ── io.seek ───────────────────────────────────────────────────
            // Seek is a no-op in the in-memory host (no stateful file handles).
            ("io.seek", "seek") => Ok(StdlibValue::Unit),

            // ── network ───────────────────────────────────────────────────
            // Network operations require a real network host; stubs return
            // Unit so tests that only verify routing do not panic.
            ("network.connect", "connect") => Ok(StdlibValue::Unit),
            ("network.connect", "send") => Ok(StdlibValue::Int(0)),
            ("network.connect", "receive") => Ok(StdlibValue::Bytes(vec![])),
            ("network.bind", "listen") => Ok(StdlibValue::Unit),

            // ── http ──────────────────────────────────────────────────────
            ("http.call", "request") => Ok(StdlibValue::Unit),
            ("http.serve", "serve") => Ok(StdlibValue::Unit),

            // ── process ───────────────────────────────────────────────────
            ("process.spawn", "spawn") => Ok(StdlibValue::Unit),
            ("process.wait", "wait") => Ok(StdlibValue::Int(0)),
            ("process.signal", "kill") => Ok(StdlibValue::Unit),

            _ => Err(StdlibExecError::CapabilityRequired {
                capability: capability.to_string(),
                operation: operation.to_string(),
            }),
        }
    }
}

fn path_arg(arg: Option<&StdlibValue>) -> Result<&str, StdlibExecError> {
    match arg.ok_or(StdlibExecError::Arity {
        expected: 1,
        actual: 0,
    })? {
        StdlibValue::Path(path) | StdlibValue::Text(path) => Ok(path),
        _ => Err(StdlibExecError::Type {
            expected: "Path or Text",
        }),
    }
}
