// ── ail-stdlib::env ───────────────────────────────────────────────────────
//
// Environment variable access for the AIL `std.env` module.
//
// # Capabilities (from docs/stdlib.md)
//
// - env.read
// - env.write
//
// # Rules
//
// - process/env are sensitive capabilities
// - prod/critical require strict grants

// ── EnvVar ────────────────────────────────────────────────────────────────

/// An environment variable key-value pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

impl EnvVar {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

// ── EnvError ──────────────────────────────────────────────────────────────

/// Error from environment operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvError {
    PermissionDenied,
    NotFound(String),
    InvalidValue(String),
    Other(String),
}

impl std::fmt::Display for EnvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvError::PermissionDenied => write!(f, "env permission denied"),
            EnvError::NotFound(k) => write!(f, "env var not found: {k}"),
            EnvError::InvalidValue(k) => write!(f, "invalid env value: {k}"),
            EnvError::Other(msg) => write!(f, "env error: {msg}"),
        }
    }
}
impl std::error::Error for EnvError {}

// ── env_read / env_write ──────────────────────────────────────────────────

/// Read an environment variable (requires `env.read` capability).
///
/// This function reads the actual host environment. In AIL, this call must
/// be mediated by the runtime capability system in production.
pub fn env_read(key: &str) -> Result<String, EnvError> {
    std::env::var(key).map_err(|e| match e {
        std::env::VarError::NotPresent => EnvError::NotFound(key.to_string()),
        std::env::VarError::NotUnicode(_) => EnvError::InvalidValue(key.to_string()),
    })
}

/// Write an environment variable (requires `env.write` capability).
///
/// In AIL, this is a sensitive, capability-gated operation. The actual
/// `set_var` call is provided by the runtime host; this function is a
/// stub that returns `Ok(())` to satisfy the API contract without calling
/// the OS directly from the stdlib layer.
pub fn env_write(_key: &str, _value: &str) -> Result<(), EnvError> {
    // Stub: in production AIL code, the runtime host injects the actual
    // implementation via the `env.write` capability binding.
    Ok(())
}

/// List all environment variables (requires `env.read` capability).
pub fn env_list() -> Vec<EnvVar> {
    std::env::vars().map(|(k, v)| EnvVar::new(k, v)).collect()
}
