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
/// Sets `key` to `value` in the current process environment via
/// [`std::env::set_var`].  Returns `Err(EnvError::InvalidValue)` if `key`
/// is empty, contains `'='`, or contains a NUL byte, which would produce
/// an OS-level error or silently corrupt the environment.
///
/// # Safety
///
/// This function is safe to call but mutates process-global state.  In AIL
/// production code the capability system gates access; callers must hold the
/// `env.write` capability before invoking this function.
#[allow(unsafe_code)]
pub fn env_write(key: &str, value: &str) -> Result<(), EnvError> {
    if key.is_empty() || key.contains('=') || key.contains('\0') || value.contains('\0') {
        return Err(EnvError::InvalidValue(key.to_string()));
    }
    // SAFETY: key and value have been validated above — no NUL bytes,
    // no '=' in key, key is non-empty.  The AIL capability system ensures
    // `env.write` is held before this function is reachable.
    unsafe { std::env::set_var(key, value) };
    Ok(())
}

/// List all environment variables (requires `env.read` capability).
pub fn env_list() -> Vec<EnvVar> {
    std::env::vars().map(|(k, v)| EnvVar::new(k, v)).collect()
}
