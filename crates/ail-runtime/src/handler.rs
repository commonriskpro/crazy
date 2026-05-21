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

use crate::abi::{HostError, HostResult};
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
