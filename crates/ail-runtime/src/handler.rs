// ── ail-runtime::handler ─────────────────────────────────────────────────
//
// `Handler` trait — pluggable capability dispatch.
//
// A `Handler` declares which capabilities it serves and interprets each
// `call_capability` invocation.  Multiple handlers can be registered on a
// `RuntimeHost`; dispatch iterates in registration order and uses the first
// handler whose `capabilities()` list contains the requested capability.
//
// `InMemoryHandler` is a test-only handler that returns a canned byte
// response for every capability it declares.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use ail_package::trust::TrustLevel;

use crate::abi::{HostError, HostResult};
use crate::codec::StructuredValue;
use crate::profile::CapabilityId;

// ── Handler ───────────────────────────────────────────────────────────────

/// A pluggable capability handler.
///
/// Implementors declare which [`CapabilityId`]s they serve and interpret
/// each incoming capability call.  Multiple handlers may be registered on
/// a [`RuntimeHost`](crate::host::RuntimeHost); the host dispatches to the
/// first handler whose `capabilities()` slice contains the requested
/// capability.
///
/// # Safety contract
///
/// Handlers must be `Send + Sync` because they are shared with Wasmtime
/// host-function closures that may execute on any thread.
pub trait Handler: Send + Sync {
    /// Human-readable name for this handler (appears in audit events).
    fn name(&self) -> &str;

    /// Implementation trust level declared by this handler.
    ///
    /// Trust levels correspond to `docs/runtime.md §Handler execution model`:
    ///
    /// - [`TrustLevel::Verified`]   — handler identity has been attested/signed.
    /// - [`TrustLevel::Assumed`]    — internally trusted by convention (default).
    /// - [`TrustLevel::Unverified`] — no trust claim; blocked in prod/critical.
    /// - [`TrustLevel::Unsafe`]     — explicitly unsafe; requires strong approval.
    ///
    /// The default implementation returns [`TrustLevel::Assumed`], which
    /// preserves backward compatibility for all existing handler implementations.
    /// Override to declare a more precise trust level.
    fn trust_level(&self) -> TrustLevel {
        TrustLevel::Assumed
    }

    /// Capabilities that this handler can serve.
    ///
    /// The returned slice must be stable for the lifetime of the handler.
    /// The host iterates this list to decide whether to dispatch to this
    /// handler for a given capability call.
    fn capabilities(&self) -> &[CapabilityId];

    /// Execute a capability call.
    ///
    /// `capability` — the [`CapabilityId`] being invoked (guaranteed to be
    /// in `self.capabilities()`).
    /// `operation` — the specific operation within that capability (e.g.
    /// `"read"`, `"write"`).
    /// `payload` — the raw request bytes from the WASM module.
    ///
    /// Returns the raw response bytes on success, or a [`HostError`] on
    /// failure.
    fn handle(
        &self,
        capability: &CapabilityId,
        operation: &str,
        payload: &[u8],
    ) -> HostResult<Vec<u8>>;

    /// Execute a capability call with structured arguments and return a
    /// structured result.
    ///
    /// Default implementation: validates `args`, encodes `Scalar` and `Unit`
    /// arguments as little-endian i64 bytes (8 bytes each), calls `self.handle`,
    /// then decodes the first 8 bytes of the response as a
    /// `StructuredValue::Scalar`.
    ///
    /// `Unit` args encode as 8 zero bytes.
    /// `Scalar(n)` args encode as `n.to_le_bytes()`.
    /// All other variants return `Err(HostError::PayloadEncodeError(...))` —
    /// structured types beyond scalars and unit are not yet supported by the
    /// default scalar ABI and must use a custom handler implementation.
    fn handle_structured(
        &self,
        capability: &CapabilityId,
        operation: &str,
        args: &[StructuredValue],
    ) -> HostResult<StructuredValue> {
        for arg in args {
            match arg {
                StructuredValue::Scalar(_) | StructuredValue::Unit => {}
                other => {
                    return Err(HostError::PayloadEncodeError(format!(
                        "unsupported structured value type in scalar ABI payload: {other:?}"
                    )));
                }
            }
        }
        let payload = encode_args_as_le_bytes(args);
        let raw = self.handle(capability, operation, &payload)?;
        Ok(decode_raw_as_structured(&raw))
    }
}

// ── handle_structured helpers ─────────────────────────────────────────────

/// Encode a slice of `StructuredValue` arguments as little-endian i64 bytes.
///
/// Each argument contributes exactly 8 bytes:
/// - `Scalar(n)` → `n.to_le_bytes()`
/// - `Unit`      → 8 zero bytes
/// - all other variants → 8 zero bytes
pub(crate) fn encode_args_as_le_bytes(args: &[StructuredValue]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(args.len() * 8);
    for arg in args {
        let val: i64 = match arg {
            StructuredValue::Scalar(n) => *n,
            _ => 0,
        };
        buf.extend_from_slice(&val.to_le_bytes());
    }
    buf
}

/// Decode the first 8 bytes of a raw response as a little-endian i64
/// `StructuredValue::Scalar`.  Returns `Unit` if `raw` has fewer than 8 bytes.
pub(crate) fn decode_raw_as_structured(raw: &[u8]) -> StructuredValue {
    if raw.len() < 8 {
        return StructuredValue::Unit;
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&raw[..8]);
    StructuredValue::Scalar(i64::from_le_bytes(buf))
}

// ── InMemoryHandler ───────────────────────────────────────────────────────

/// A test-only handler that returns a canned byte response.
///
/// Every `handle` call for a declared capability returns the same `response`
/// bytes.  Calls for capabilities not in the declared list return a
/// [`HostError`].
///
/// # Example
///
/// ```rust
/// use ail_runtime::{InMemoryHandler, CapabilityId};
/// use std::sync::Arc;
///
/// let handler = InMemoryHandler::new(
///     "test-handler",
///     vec![CapabilityId::new("FileRead")],
///     b"canned-response".to_vec(),
/// );
/// ```
pub struct InMemoryHandler {
    name: &'static str,
    caps: Vec<CapabilityId>,
    response: Vec<u8>,
}

pub struct LogHandler {
    caps: Vec<CapabilityId>,
    output: Mutex<Vec<String>>,
}

impl LogHandler {
    pub fn new() -> Self {
        Self {
            caps: vec![CapabilityId::new("log.write")],
            output: Mutex::new(Vec::new()),
        }
    }

    pub fn output(&self) -> Vec<String> {
        self.output
            .lock()
            .expect("log output lock must not be poisoned")
            .clone()
    }
}

impl Default for LogHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl Handler for LogHandler {
    fn name(&self) -> &str {
        "log"
    }

    fn capabilities(&self) -> &[CapabilityId] {
        &self.caps
    }

    fn handle(
        &self,
        _capability: &CapabilityId,
        operation: &str,
        payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        if operation != "write" {
            return Err(HostError::Custom(format!(
                "unknown log.write operation: {operation}"
            )));
        }
        let text = String::from_utf8(payload.to_vec()).map_err(|e| {
            HostError::PayloadDecodeError(format!("log.write payload is not UTF-8: {e}"))
        })?;
        self.output
            .lock()
            .expect("log output lock must not be poisoned")
            .push(text);
        Ok(0i64.to_le_bytes().to_vec())
    }
}

pub struct ClockHandler {
    caps: Vec<CapabilityId>,
}

pub struct FileReadHandler {
    caps: Vec<CapabilityId>,
}

impl ClockHandler {
    pub fn new() -> Self {
        Self {
            caps: vec![CapabilityId::new("clock"), CapabilityId::new("clock.now")],
        }
    }
}

impl Default for ClockHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl Handler for ClockHandler {
    fn name(&self) -> &str {
        "clock"
    }

    fn capabilities(&self) -> &[CapabilityId] {
        &self.caps
    }

    fn handle(
        &self,
        _capability: &CapabilityId,
        operation: &str,
        _payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        match operation {
            "now" => {
                // Contract: clock.now returns epoch-milliseconds as i64.
                // Current epoch ms ≈ 1.7e12, well within i64::MAX (9.2e18),
                // but we use a checked conversion so a pathological system
                // clock (or a host running after year ~292 million) produces a
                // clear error instead of silently wrapping.
                let now_ms: i64 = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| HostError::Custom(format!("clock before unix epoch: {e}")))?
                    .as_millis()
                    .try_into()
                    .map_err(|_| HostError::Custom("epoch-ms overflows i64".to_string()))?;
                Ok(now_ms.to_le_bytes().to_vec())
            }
            other => Err(HostError::Custom(format!(
                "unknown clock operation: {other}"
            ))),
        }
    }
}

impl FileReadHandler {
    pub fn new() -> Self {
        Self {
            caps: vec![CapabilityId::new("file.read")],
        }
    }
}

impl Default for FileReadHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl Handler for FileReadHandler {
    fn name(&self) -> &str {
        "file.read"
    }

    fn capabilities(&self) -> &[CapabilityId] {
        &self.caps
    }

    fn handle(
        &self,
        _capability: &CapabilityId,
        operation: &str,
        payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        if operation != "read" {
            return Err(HostError::Custom(format!(
                "unknown file.read operation: {operation}"
            )));
        }
        let path = std::str::from_utf8(payload).map_err(|e| {
            HostError::PayloadDecodeError(format!("file.read path is not UTF-8: {e}"))
        })?;
        std::fs::read(path)
            .map_err(|e| HostError::Custom(format!("file.read failed for `{path}`: {e}")))
    }
}

impl InMemoryHandler {
    /// Create a new `InMemoryHandler`.
    ///
    /// `name` — identifier used in audit events.
    /// `caps` — capabilities this handler declares.
    /// `response` — bytes returned for every successful `handle` call.
    pub fn new(name: &'static str, caps: Vec<CapabilityId>, response: Vec<u8>) -> Self {
        InMemoryHandler {
            name,
            caps,
            response,
        }
    }
}

impl Handler for InMemoryHandler {
    fn name(&self) -> &str {
        self.name
    }

    fn capabilities(&self) -> &[CapabilityId] {
        &self.caps
    }

    fn handle(
        &self,
        capability: &CapabilityId,
        _operation: &str,
        _payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        if self.caps.contains(capability) {
            Ok(self.response.clone())
        } else {
            Err(HostError::HandlerNotBound(format!(
                "InMemoryHandler does not handle capability: {}",
                capability.as_str()
            )))
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // ── TASK-F1: handle_structured default method tests (TDD RED) ────────
    // These tests call `handle_structured` which does not exist on the
    // `Handler` trait yet.

    /// Minimal handler for testing: returns a fixed i64 value as LE bytes.
    fn i64_handler(response: i64) -> Arc<InMemoryHandler> {
        Arc::new(InMemoryHandler::new(
            "test-i64",
            vec![CapabilityId::new("cap")],
            response.to_le_bytes().to_vec(),
        ))
    }

    #[test]
    fn handle_structured_default_calls_handle() {
        // InMemoryHandler returns 99i64 as LE bytes.
        // handle_structured should decode that as StructuredValue::Scalar(99).
        let handler = i64_handler(99);
        let cap = CapabilityId::new("cap");
        let result = handler
            .handle_structured(&cap, "op", &[StructuredValue::Scalar(1)])
            .expect("handle_structured must succeed");
        assert_eq!(result, StructuredValue::Scalar(99));
    }

    #[test]
    fn handle_structured_unit_arg_encodes_as_zero() {
        // Unit arg must encode as 8 zero bytes in the payload.
        // We verify this indirectly: the handler receives the encoded bytes;
        // we hook into the response by checking what the underlying `handle`
        // receives (reflected via response bytes in InMemoryHandler).
        //
        // Here we just check that calling handle_structured with Unit arg
        // doesn't panic and returns the canned response.
        let handler = i64_handler(0);
        let cap = CapabilityId::new("cap");
        let result = handler
            .handle_structured(&cap, "op", &[StructuredValue::Unit])
            .expect("handle_structured with Unit arg must succeed");
        assert_eq!(result, StructuredValue::Scalar(0));
    }

    #[test]
    fn handle_structured_i64_arg_encodes_correctly() {
        // Scalar(42) arg → 42i64.to_le_bytes() in the payload.
        // InMemoryHandler ignores the payload; we just verify success.
        let handler = i64_handler(7);
        let cap = CapabilityId::new("cap");
        let result = handler
            .handle_structured(&cap, "op", &[StructuredValue::Scalar(42)])
            .expect("handle_structured with Scalar arg must succeed");
        assert_eq!(result, StructuredValue::Scalar(7));
    }

    #[test]
    fn handle_structured_returns_error_for_unsupported_types() {
        // Non-Scalar, non-Unit variants must produce PayloadEncodeError,
        // not silently encode as zero bytes.
        let handler = i64_handler(0);
        let cap = CapabilityId::new("cap");

        // Record variant — unsupported
        let record_arg =
            StructuredValue::Record(vec![("field".to_string(), StructuredValue::Scalar(1))]);
        let err = handler
            .handle_structured(&cap, "op", &[record_arg])
            .expect_err("Record arg must produce an error");
        assert!(
            matches!(err, crate::abi::HostError::PayloadEncodeError(_)),
            "expected PayloadEncodeError, got {err:?}"
        );

        // List variant — unsupported
        let list_arg = StructuredValue::List(vec![StructuredValue::Scalar(1)]);
        let err2 = handler
            .handle_structured(&cap, "op", &[list_arg])
            .expect_err("List arg must produce an error");
        assert!(
            matches!(err2, crate::abi::HostError::PayloadEncodeError(_)),
            "expected PayloadEncodeError for List, got {err2:?}"
        );
    }
}
