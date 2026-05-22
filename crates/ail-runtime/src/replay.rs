// ── ail-runtime::replay ───────────────────────────────────────────────────
//
// Deterministic handlers and replay engine (G29).
//
// Per runtime.md §"Determinism, replay and testing":
//   Profiles can use deterministic handlers:
//     FixedClock, SeededRandom, RecordedHttp, InMemoryDb, FakePayment
//
//   Replay mode:
//     replay trace_id=trace_123
//       use recorded capability responses
//       verify same output hashes
//     end
//
// All handlers implement the `Handler` trait and declare their capabilities
// so they can be registered with a `RuntimeHost`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::abi::{HostError, HostResult};
use crate::handler::Handler;
use crate::profile::CapabilityId;

// ── FixedClock ────────────────────────────────────────────────────────────

/// Deterministic clock handler that returns a fixed timestamp.
///
/// Every `handle` call for `clock.now` returns the same configured
/// `timestamp_ms` value encoded as a little-endian `u64` (8 bytes).
/// This makes time a deterministic capability for replay/test profiles.
///
/// # Example
///
/// ```rust
/// use ail_runtime::replay::FixedClock;
/// use ail_runtime::handler::Handler;
/// use ail_runtime::profile::CapabilityId;
///
/// let clock = FixedClock::new(1_700_000_000_000);
/// let cap = CapabilityId::new("clock.now");
/// let bytes = clock.handle(&cap, "now", &[]).unwrap();
/// let ts = u64::from_le_bytes(bytes.try_into().unwrap());
/// assert_eq!(ts, 1_700_000_000_000);
/// ```
pub struct FixedClock {
    timestamp_ms: u64,
    caps: Vec<CapabilityId>,
}

impl FixedClock {
    /// Create a `FixedClock` that always returns `timestamp_ms`.
    pub fn new(timestamp_ms: u64) -> Self {
        FixedClock {
            timestamp_ms,
            caps: vec![CapabilityId::new("clock.now")],
        }
    }
}

impl Handler for FixedClock {
    fn name(&self) -> &str {
        "FixedClock"
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
        Ok(self.timestamp_ms.to_le_bytes().to_vec())
    }
}

// ── SeededRandom ──────────────────────────────────────────────────────────

/// Deterministic random handler seeded with a fixed value.
///
/// Uses a simple xorshift64 PRNG — deterministic, portable, and free of
/// external dependencies.  Each call advances the internal state atomically
/// so consecutive calls produce different values while remaining
/// reproducible from the same seed.
///
/// Returns 8 bytes (little-endian `u64`) per call.
pub struct SeededRandom {
    state: AtomicU64,
    caps: Vec<CapabilityId>,
}

impl SeededRandom {
    /// Create a `SeededRandom` with the given initial seed.
    pub fn new(seed: u64) -> Self {
        // Ensure seed != 0 for xorshift (0 is a fixed point).
        let initial = if seed == 0 { 1 } else { seed };
        SeededRandom {
            state: AtomicU64::new(initial),
            caps: vec![CapabilityId::new("random.next_u64")],
        }
    }

    /// Advance the PRNG state and return the next value (xorshift64).
    fn next(&self) -> u64 {
        loop {
            let old = self.state.load(Ordering::Relaxed);
            let mut x = old;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            // CAS: if another thread raced, retry.
            if self
                .state
                .compare_exchange(old, x, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                return x;
            }
        }
    }
}

impl Handler for SeededRandom {
    fn name(&self) -> &str {
        "SeededRandom"
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
        Ok(self.next().to_le_bytes().to_vec())
    }
}

// ── RecordedHttp ──────────────────────────────────────────────────────────

/// HTTP handler that replays pre-recorded responses.
///
/// Maps `operation` strings (e.g. `"GET:https://api.example.com/prices"`)
/// to recorded byte responses.  Unknown operations return a `HostError`.
///
/// Implements `Handler` for `http.call:*` capabilities so it can be wired
/// into a `RuntimeHost` test profile.
pub struct RecordedHttp {
    recordings: HashMap<String, Vec<u8>>,
    caps: Vec<CapabilityId>,
}

impl RecordedHttp {
    /// Create an empty `RecordedHttp` handler.
    pub fn new() -> Self {
        RecordedHttp {
            recordings: HashMap::new(),
            caps: vec![CapabilityId::new("http.call:RecordedHttp")],
        }
    }

    /// Record a response for an operation key.
    ///
    /// `operation` — the operation string used by the WASM caller
    ///   (e.g. `"GET:https://api.example.com/prices"`).
    /// `response` — bytes to return when that operation is replayed.
    pub fn record(&mut self, operation: impl Into<String>, response: Vec<u8>) {
        self.recordings.insert(operation.into(), response);
    }
}

impl Default for RecordedHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl Handler for RecordedHttp {
    fn name(&self) -> &str {
        "RecordedHttp"
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
        match self.recordings.get(operation) {
            Some(resp) => Ok(resp.clone()),
            None => Err(HostError {
                message: format!("RecordedHttp: no recording for operation: {operation}"),
            }),
        }
    }
}

// ── InMemoryDb ────────────────────────────────────────────────────────────

/// In-memory key/value database handler for deterministic testing.
///
/// Supports two operation patterns:
/// - `"read:<key>"` — returns the value stored under `<key>`, or empty bytes.
/// - `"write:<key>"` — stores `payload` under `<key>`.
///
/// Implements `Handler` for both `database.read:*` and `database.write:*`
/// capabilities so it can serve as a complete in-memory DB substitute.
///
/// The internal store is wrapped in a `Mutex` so the handler can be
/// `Sync` and mutated through shared references (required by `Handler`).
pub struct InMemoryDb {
    store: Mutex<HashMap<String, Vec<u8>>>,
    caps: Vec<CapabilityId>,
}

impl InMemoryDb {
    /// Create an empty `InMemoryDb`.
    pub fn new() -> Self {
        InMemoryDb {
            store: Mutex::new(HashMap::new()),
            caps: vec![
                CapabilityId::new("database.read:InMemoryDb"),
                CapabilityId::new("database.write:InMemoryDb"),
            ],
        }
    }

    /// Pre-populate a record.
    ///
    /// Useful for test setup: `db.insert("Cart:42", data)`.
    pub fn insert(&self, key: impl Into<String>, value: Vec<u8>) {
        self.store.lock().unwrap().insert(key.into(), value);
    }
}

impl Default for InMemoryDb {
    fn default() -> Self {
        Self::new()
    }
}

impl Handler for InMemoryDb {
    fn name(&self) -> &str {
        "InMemoryDb"
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
        // Operation format: "read:<key>" or "write:<key>"
        if let Some(key) = operation.strip_prefix("read:") {
            let store = self.store.lock().unwrap();
            Ok(store.get(key).cloned().unwrap_or_default())
        } else if let Some(key) = operation.strip_prefix("write:") {
            let mut store = self.store.lock().unwrap();
            store.insert(key.to_string(), payload.to_vec());
            Ok(vec![])
        } else {
            Err(HostError {
                message: format!(
                    "InMemoryDb: unknown operation `{operation}`. Expected `read:<key>` or `write:<key>`"
                ),
            })
        }
    }
}

// ── FakePayment ───────────────────────────────────────────────────────────

/// Test payment handler that succeeds or fails based on configuration.
///
/// In test profiles, `payment.charge:*` is bound to `FakePayment` instead
/// of a real payment provider.  This ensures deterministic, network-free
/// test execution.
///
/// When `succeed = false`, returns a `HostError` with `"PaymentDeclined"`.
pub struct FakePayment {
    succeed: bool,
    response: Vec<u8>,
    caps: Vec<CapabilityId>,
}

impl FakePayment {
    /// Create a `FakePayment` handler.
    ///
    /// `succeed` — if `true`, returns `response`; if `false`, returns an error.
    /// `response` — bytes returned on success.
    pub fn new(succeed: bool, response: Vec<u8>) -> Self {
        FakePayment {
            succeed,
            response,
            caps: vec![CapabilityId::new("payment.charge:FakePayment")],
        }
    }
}

impl Handler for FakePayment {
    fn name(&self) -> &str {
        "FakePayment"
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
        if self.succeed {
            Ok(self.response.clone())
        } else {
            Err(HostError {
                message: "PaymentDeclined: FakePayment configured to decline".to_string(),
            })
        }
    }
}

// ── ReplayEngine ──────────────────────────────────────────────────────────

/// Records capability call responses and produces a handler that replays them.
///
/// ## Usage
///
/// 1. Record responses during a "capture" run (or pre-configure them).
/// 2. Call `into_handler()` to convert the engine into a `Handler` that
///    replays the recorded responses.
/// 3. Register the handler with a `RuntimeHost` in replay/test mode.
///
/// # Example
///
/// ```rust
/// use ail_runtime::profile::CapabilityId;
/// use ail_runtime::replay::ReplayEngine;
/// use ail_runtime::handler::Handler;
///
/// let mut engine = ReplayEngine::new();
/// let cap = CapabilityId::new("database.read:Cart");
/// engine.record(cap.clone(), "read:Cart:42", b"cart-data".to_vec());
///
/// let handler = engine.into_handler();
/// let result = handler.handle(&cap, "read:Cart:42", &[]).unwrap();
/// assert_eq!(result, b"cart-data");
/// ```
pub struct ReplayEngine {
    recordings: Vec<(CapabilityId, String, Vec<u8>)>,
}

impl ReplayEngine {
    /// Create an empty `ReplayEngine`.
    pub fn new() -> Self {
        ReplayEngine {
            recordings: Vec::new(),
        }
    }

    /// Record a capability response.
    ///
    /// `capability` — the capability being called.
    /// `operation` — the operation string.
    /// `response` — bytes to replay for this (capability, operation) pair.
    pub fn record(
        &mut self,
        capability: CapabilityId,
        operation: impl Into<String>,
        response: Vec<u8>,
    ) {
        self.recordings
            .push((capability, operation.into(), response));
    }

    /// Consume the engine and produce a [`Handler`] that replays recordings.
    pub fn into_handler(self) -> ReplayHandler {
        let map: HashMap<(String, String), Vec<u8>> = self
            .recordings
            .into_iter()
            .map(|(cap, op, resp)| ((cap.as_str().to_string(), op), resp))
            .collect();

        // Collect unique capabilities declared.
        let mut seen = std::collections::HashSet::new();
        let caps: Vec<CapabilityId> = {
            let mut v = Vec::new();
            // We need the original cap IDs — rebuild from map keys.
            for (cap_str, _) in map.keys() {
                if seen.insert(cap_str.clone()) {
                    v.push(CapabilityId::new(cap_str.clone()));
                }
            }
            // Always include a generic replay capability so the handler can
            // be registered even when no recordings exist.
            if v.is_empty() {
                v.push(CapabilityId::new("replay.unregistered"));
            }
            v
        };

        ReplayHandler {
            map: Arc::new(map),
            caps,
        }
    }
}

impl Default for ReplayEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── ReplayHandler (internal) ──────────────────────────────────────────────

/// Handler produced by [`ReplayEngine::into_handler`].
///
/// Replays pre-recorded capability responses deterministically.
pub struct ReplayHandler {
    map: Arc<HashMap<(String, String), Vec<u8>>>,
    caps: Vec<CapabilityId>,
}

impl Handler for ReplayHandler {
    fn name(&self) -> &str {
        "ReplayHandler"
    }

    fn capabilities(&self) -> &[CapabilityId] {
        &self.caps
    }

    fn handle(
        &self,
        capability: &CapabilityId,
        operation: &str,
        _payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        let key = (capability.as_str().to_string(), operation.to_string());
        match self.map.get(&key) {
            Some(resp) => Ok(resp.clone()),
            None => Err(HostError {
                message: format!(
                    "ReplayHandler: no recording for capability=`{}` operation=`{}`",
                    capability.as_str(),
                    operation
                ),
            }),
        }
    }
}
