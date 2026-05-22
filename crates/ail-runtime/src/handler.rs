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

use std::time::{SystemTime, UNIX_EPOCH};

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
    /// Default implementation: encodes `args` as little-endian i64 bytes
    /// (8 bytes per argument), calls `self.handle`, then decodes the first
    /// 8 bytes of the response as a little-endian i64 `StructuredValue::Scalar`.
    ///
    /// `Unit` args encode as 8 zero bytes.
    /// `Scalar(n)` args encode as `n.to_le_bytes()`.
    /// All other variants encode as 8 zero bytes.
    fn handle_structured(
        &self,
        capability: &CapabilityId,
        operation: &str,
        args: &[StructuredValue],
    ) -> HostResult<StructuredValue> {
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
}

impl LogHandler {
    pub fn new() -> Self {
        Self {
            caps: vec![CapabilityId::new("log")],
        }
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
        let args = decode_i64_args(payload);
        println!("ail log.{operation}: {args:?}");
        Ok(0i64.to_le_bytes().to_vec())
    }
}

pub struct ClockHandler {
    caps: Vec<CapabilityId>,
}

impl ClockHandler {
    pub fn new() -> Self {
        Self {
            caps: vec![CapabilityId::new("clock")],
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
        _operation: &str,
        _payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| HostError {
                message: format!("clock before unix epoch: {e}"),
            })?
            .as_secs() as i64;
        Ok(now.to_le_bytes().to_vec())
    }
}

fn decode_i64_args(payload: &[u8]) -> Vec<i64> {
    payload
        .chunks_exact(8)
        .map(|chunk| {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(chunk);
            i64::from_le_bytes(buf)
        })
        .collect()
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
                Err(HostError {
                    message: format!(
                        "InMemoryHandler does not handle capability: {}",
                        capability.as_str()
                    ),
                })
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use super::*;

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
}

