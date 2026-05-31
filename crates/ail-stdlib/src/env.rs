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

// ── Env capability contracts ──────────────────────────────────────────────

/// Environment capability labels exposed by `std.env` operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvCapability {
    Read,
    Write,
}

impl EnvCapability {
    pub fn label(self) -> &'static str {
        match self {
            EnvCapability::Read => "env.read",
            EnvCapability::Write => "env.write",
        }
    }
}

impl std::fmt::Display for EnvCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Operation families for environment access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvOperationKind {
    Read,
    Write,
    List,
}

impl EnvOperationKind {
    pub fn label(self) -> &'static str {
        match self {
            EnvOperationKind::Read => "read_var",
            EnvOperationKind::Write => "write_var",
            EnvOperationKind::List => "list_vars",
        }
    }

    pub fn required_capability(self) -> EnvCapability {
        match self {
            EnvOperationKind::Read | EnvOperationKind::List => EnvCapability::Read,
            EnvOperationKind::Write => EnvCapability::Write,
        }
    }
}

/// Stable, redacted taxonomy for environment variable keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvKeyShape {
    Valid,
    Empty,
    ContainsEquals,
    ContainsNul,
}

impl EnvKeyShape {
    pub fn label(self) -> &'static str {
        match self {
            EnvKeyShape::Valid => "valid",
            EnvKeyShape::Empty => "empty",
            EnvKeyShape::ContainsEquals => "contains-equals",
            EnvKeyShape::ContainsNul => "contains-nul",
        }
    }

    pub fn is_valid(self) -> bool {
        matches!(self, EnvKeyShape::Valid)
    }
}

/// Stable, redacted taxonomy for environment variable values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvValueShape {
    Present,
    Empty,
    ContainsNul,
}

impl EnvValueShape {
    pub fn label(self) -> &'static str {
        match self {
            EnvValueShape::Present => "present",
            EnvValueShape::Empty => "empty",
            EnvValueShape::ContainsNul => "contains-nul",
        }
    }

    pub fn is_valid(self) -> bool {
        !matches!(self, EnvValueShape::ContainsNul)
    }
}

/// Redacted shape error for env contracts. Does not echo keys or values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvShapeError {
    pub kind: EnvOperationKind,
    pub key_shape: Option<EnvKeyShape>,
    pub value_shape: Option<EnvValueShape>,
}

impl std::fmt::Display for EnvShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid env operation shape: kind={}", self.kind.label())?;
        if let Some(shape) = self.key_shape {
            write!(f, ", key_shape={}", shape.label())?;
        }
        if let Some(shape) = self.value_shape {
            write!(f, ", value_shape={}", shape.label())?;
        }
        Ok(())
    }
}
impl std::error::Error for EnvShapeError {}

/// Deny-by-default descriptor for a `std.env` operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvOperationDescriptor {
    pub kind: EnvOperationKind,
    pub capability: EnvCapability,
    pub capability_label: &'static str,
    pub key_shape: Option<EnvKeyShape>,
    pub value_shape: Option<EnvValueShape>,
    pub grant_required: bool,
    pub ambient_access: bool,
}

impl EnvOperationDescriptor {
    pub fn read(key: &str) -> Self {
        Self::new(EnvOperationKind::Read, Some(key), None)
    }

    pub fn write(key: &str, value: &str) -> Self {
        Self::new(EnvOperationKind::Write, Some(key), Some(value))
    }

    pub fn list() -> Self {
        Self::new(EnvOperationKind::List, None, None)
    }

    fn new(kind: EnvOperationKind, key: Option<&str>, value: Option<&str>) -> Self {
        let capability = kind.required_capability();
        Self {
            kind,
            capability,
            capability_label: capability.label(),
            key_shape: key.map(env_key_shape),
            value_shape: value.map(env_value_shape),
            grant_required: true,
            ambient_access: false,
        }
    }

    pub fn diagnostic_key(&self) -> String {
        let key_shape = self.key_shape.map(EnvKeyShape::label).unwrap_or("no-key");
        let value_shape = self
            .value_shape
            .map(EnvValueShape::label)
            .unwrap_or("no-value");
        format!(
            "std.env.{}:{}:{}:{}",
            self.kind.label(),
            self.capability_label,
            key_shape,
            value_shape
        )
    }

    pub fn validate_shape(&self) -> Result<(), EnvShapeError> {
        if self.key_shape.map_or(false, |shape| !shape.is_valid())
            || self.value_shape.map_or(false, |shape| !shape.is_valid())
        {
            Err(EnvShapeError {
                kind: self.kind,
                key_shape: self.key_shape,
                value_shape: self.value_shape,
            })
        } else {
            Ok(())
        }
    }
}

pub fn env_key_shape(key: &str) -> EnvKeyShape {
    if key.is_empty() {
        EnvKeyShape::Empty
    } else if key.contains('\0') {
        EnvKeyShape::ContainsNul
    } else if key.contains('=') {
        EnvKeyShape::ContainsEquals
    } else {
        EnvKeyShape::Valid
    }
}

pub fn env_value_shape(value: &str) -> EnvValueShape {
    if value.contains('\0') {
        EnvValueShape::ContainsNul
    } else if value.is_empty() {
        EnvValueShape::Empty
    } else {
        EnvValueShape::Present
    }
}

pub fn validate_env_read_contract(key: &str) -> Result<(), EnvShapeError> {
    EnvOperationDescriptor::read(key).validate_shape()
}

pub fn validate_env_write_contract(key: &str, value: &str) -> Result<(), EnvShapeError> {
    EnvOperationDescriptor::write(key, value).validate_shape()
}

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
    validate_env_write_contract(key, value).map_err(|_| EnvError::InvalidValue(key.to_string()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_capability_labels_are_stable() {
        assert_eq!(EnvCapability::Read.label(), "env.read");
        assert_eq!(EnvCapability::Write.label(), "env.write");
        assert_eq!(EnvCapability::Read.to_string(), "env.read");
    }

    #[test]
    fn env_descriptors_make_denied_by_default_semantics_visible() {
        let read = EnvOperationDescriptor::read("DATABASE_URL");
        let write = EnvOperationDescriptor::write("FEATURE_FLAG", "enabled");
        let list = EnvOperationDescriptor::list();

        assert_eq!(read.kind, EnvOperationKind::Read);
        assert_eq!(read.capability, EnvCapability::Read);
        assert_eq!(read.capability_label, "env.read");
        assert_eq!(read.key_shape, Some(EnvKeyShape::Valid));
        assert_eq!(read.value_shape, None);
        assert!(read.grant_required);
        assert!(!read.ambient_access);
        assert_eq!(read.validate_shape(), Ok(()));
        assert_eq!(
            read.diagnostic_key(),
            "std.env.read_var:env.read:valid:no-value"
        );

        assert_eq!(write.kind, EnvOperationKind::Write);
        assert_eq!(write.capability, EnvCapability::Write);
        assert_eq!(write.capability_label, "env.write");
        assert_eq!(write.value_shape, Some(EnvValueShape::Present));
        assert_eq!(
            write.diagnostic_key(),
            "std.env.write_var:env.write:valid:present"
        );

        assert_eq!(list.kind, EnvOperationKind::List);
        assert_eq!(list.capability, EnvCapability::Read);
        assert_eq!(list.capability_label, "env.read");
        assert_eq!(list.key_shape, None);
        assert_eq!(list.value_shape, None);
        assert_eq!(
            list.diagnostic_key(),
            "std.env.list_vars:env.read:no-key:no-value"
        );
    }

    #[test]
    fn env_shapes_are_stable_and_redacted() {
        assert_eq!(env_key_shape(""), EnvKeyShape::Empty);
        assert_eq!(env_key_shape("BAD=KEY"), EnvKeyShape::ContainsEquals);
        assert_eq!(env_key_shape("BAD\0KEY"), EnvKeyShape::ContainsNul);
        assert_eq!(env_key_shape("GOOD_KEY"), EnvKeyShape::Valid);

        assert_eq!(env_value_shape(""), EnvValueShape::Empty);
        assert_eq!(env_value_shape("secret-token"), EnvValueShape::Present);
        assert_eq!(env_value_shape("secret\0token"), EnvValueShape::ContainsNul);
    }

    #[test]
    fn env_contract_validation_reports_shapes_without_leaking_inputs() {
        let bad_key = EnvOperationDescriptor::read("SECRET=TOKEN");
        let bad_value = EnvOperationDescriptor::write("API_TOKEN", "abc\0def");

        assert_eq!(
            bad_key.validate_shape(),
            Err(EnvShapeError {
                kind: EnvOperationKind::Read,
                key_shape: Some(EnvKeyShape::ContainsEquals),
                value_shape: None,
            })
        );
        assert_eq!(
            bad_key.validate_shape().unwrap_err().to_string(),
            "invalid env operation shape: kind=read_var, key_shape=contains-equals"
        );

        assert_eq!(
            bad_value.validate_shape(),
            Err(EnvShapeError {
                kind: EnvOperationKind::Write,
                key_shape: Some(EnvKeyShape::Valid),
                value_shape: Some(EnvValueShape::ContainsNul),
            })
        );
        assert_eq!(
            validate_env_write_contract("API_TOKEN", "abc\0def")
                .unwrap_err()
                .to_string(),
            "invalid env operation shape: kind=write_var, key_shape=valid, value_shape=contains-nul"
        );
        assert_eq!(validate_env_read_contract("GOOD_KEY"), Ok(()));
    }
}
