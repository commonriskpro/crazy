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
//
// `ReplayEngine` records responses AND their BLAKE3 output hashes.
// `into_verifying_handler()` produces a handler that replays AND verifies.
// `verify(cap, op, actual)` compares actual bytes against the recorded hash.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

type ReplayResponseMap = HashMap<(String, String), (Vec<u8>, String)>;

use crate::abi::{HostError, HostResult};
use crate::audit::{
    REPLAY_MISMATCH_DIAGNOSTIC_KEY_HASH_MISMATCH, REPLAY_MISMATCH_DIAGNOSTIC_KEY_MISSING_RECORDING,
    REPLAY_MISMATCH_HASH_MISMATCH, REPLAY_MISMATCH_MISSING_RECORDING,
};
use crate::handler::Handler;
use crate::profile::CapabilityId;

// ── ReplayVerificationError ───────────────────────────────────────────────

/// Stable replay verification failure kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayMismatchKind {
    /// No recording exists for the requested capability/operation pair.
    MissingRecording,
    /// A recording exists, but its replayed bytes hash to a different digest.
    HashMismatch,
}

impl ReplayMismatchKind {
    /// Stable audit/diagnostic category for this mismatch kind.
    pub fn category(self) -> &'static str {
        match self {
            ReplayMismatchKind::MissingRecording => REPLAY_MISMATCH_MISSING_RECORDING,
            ReplayMismatchKind::HashMismatch => REPLAY_MISMATCH_HASH_MISMATCH,
        }
    }

    /// Stable redacted diagnostic key for metrics, issue grouping, and support
    /// triage.
    pub fn diagnostic_key(self) -> &'static str {
        match self {
            ReplayMismatchKind::MissingRecording => {
                REPLAY_MISMATCH_DIAGNOSTIC_KEY_MISSING_RECORDING
            }
            ReplayMismatchKind::HashMismatch => REPLAY_MISMATCH_DIAGNOSTIC_KEY_HASH_MISMATCH,
        }
    }
}

/// Error returned when replay output-hash verification fails.
///
/// Diagnostics intentionally carry only stable categories, capability shapes,
/// operation shapes, and hashes.  They never echo raw operation strings because
/// operations commonly contain URLs, database keys, tenant IDs, or secret IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayVerificationError {
    /// Human-readable redacted description of the verification failure.
    pub message: String,
    /// Stable machine-readable mismatch kind.
    pub kind: ReplayMismatchKind,
    /// Stable redacted machine-readable diagnostic key.
    pub diagnostic_key: &'static str,
    /// Stable audit/diagnostic category matching [`ReplayMismatchKind::category`].
    pub category: &'static str,
    /// Redacted capability label, e.g. `"database.read:*"`.
    pub capability: String,
    /// Redacted operation shape, e.g. `"http.request"` or `"keyed.read"`.
    pub operation_shape: String,
    /// Recorded output hash when available.
    pub recorded_hash: Option<String>,
    /// Actual output hash when available.
    pub actual_hash: Option<String>,
}

impl ReplayVerificationError {
    fn missing_recording(capability: &CapabilityId, operation: &str) -> Self {
        Self::new(
            ReplayMismatchKind::MissingRecording,
            capability,
            operation,
            None,
            None,
        )
    }

    fn hash_mismatch(
        capability: &CapabilityId,
        operation: &str,
        recorded_hash: String,
        actual_hash: String,
    ) -> Self {
        Self::new(
            ReplayMismatchKind::HashMismatch,
            capability,
            operation,
            Some(recorded_hash),
            Some(actual_hash),
        )
    }

    fn new(
        kind: ReplayMismatchKind,
        capability: &CapabilityId,
        operation: &str,
        recorded_hash: Option<String>,
        actual_hash: Option<String>,
    ) -> Self {
        let category = kind.category();
        let diagnostic_key = kind.diagnostic_key();
        let capability = replay_capability_label(capability);
        let operation_shape = replay_operation_shape(operation).to_string();
        let mut message = format!(
            "replay verification failed: key={diagnostic_key} category={category} capability={capability} \
             operation_shape={operation_shape}"
        );
        if let (Some(recorded), Some(actual)) = (&recorded_hash, &actual_hash) {
            message.push_str(&format!(" recorded_hash={recorded} actual_hash={actual}"));
        }

        Self {
            message,
            kind,
            diagnostic_key,
            category,
            capability,
            operation_shape,
            recorded_hash,
            actual_hash,
        }
    }

    fn diagnostic_sort_key(&self) -> (&'static str, &'static str, &str, &str) {
        (
            self.diagnostic_key,
            self.category,
            self.capability.as_str(),
            self.operation_shape.as_str(),
        )
    }
}

impl std::fmt::Display for ReplayVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn replay_capability_label(capability: &CapabilityId) -> String {
    match capability.as_str().split_once(':') {
        Some((namespace, _)) => format!("{namespace}:*"),
        None => capability.as_str().to_string(),
    }
}

fn replay_operation_shape(operation: &str) -> &'static str {
    if operation.is_empty() {
        "empty"
    } else if operation.starts_with("read:") {
        "keyed.read"
    } else if operation.starts_with("write:") {
        "keyed.write"
    } else if operation.starts_with("GET:")
        || operation.starts_with("POST:")
        || operation.starts_with("PUT:")
        || operation.starts_with("PATCH:")
        || operation.starts_with("DELETE:")
        || operation.starts_with("HEAD:")
        || operation.starts_with("OPTIONS:")
    {
        "http.request"
    } else if operation.contains(':') {
        "namespaced"
    } else {
        "literal"
    }
}

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
        operation: &str,
        _payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        if operation != "now" {
            return Err(HostError::Custom(format!(
                "unknown FixedClock operation: {operation}"
            )));
        }
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
        operation: &str,
        _payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        if operation != "next_u64" {
            return Err(HostError::Custom(format!(
                "unknown SeededRandom operation: {operation}"
            )));
        }
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
            None => Err(HostError::HandlerUnavailable(format!(
                "RecordedHttp: no recording for operation: {operation}"
            ))),
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
            Err(HostError::Custom(format!(
                "InMemoryDb: unknown operation `{operation}`. Expected `read:<key>` or `write:<key>`"
            )))
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
            Err(HostError::Custom(
                "PaymentDeclined: FakePayment configured to decline".to_string(),
            ))
        }
    }
}

// ── hash helper ───────────────────────────────────────────────────────────

/// Compute a BLAKE3 hex digest of `data`.
///
/// Used both to record output hashes and to verify replayed outputs.
fn blake3_hex(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    hash.to_hex().to_string()
}

// ── ReplayEngine ──────────────────────────────────────────────────────────

/// Records capability call responses (with output hashes) and produces
/// handlers that replay or verify them.
///
/// ## Usage
///
/// 1. Record responses during a "capture" run (or pre-configure them).
/// 2. Call `into_handler()` to get a `Handler` that replays without verification.
/// 3. Call `into_verifying_handler()` to get a `Handler` that replays AND
///    checks that each response matches its recorded BLAKE3 output hash.
/// 4. Call `verify(cap, op, actual)` to check one response against its hash.
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
    /// (cap_str, operation) → (response, output_hash)
    recordings: Vec<(CapabilityId, String, Vec<u8>, String)>,
}

impl ReplayEngine {
    /// Create an empty `ReplayEngine`.
    pub fn new() -> Self {
        ReplayEngine {
            recordings: Vec::new(),
        }
    }

    /// Record a capability response AND compute its BLAKE3 output hash.
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
        let hash = blake3_hex(&response);
        self.recordings
            .push((capability, operation.into(), response, hash));
    }

    /// Compute a BLAKE3 hex digest of `data`.
    ///
    /// Public so tests can verify hash consistency.
    pub fn hash_of(data: &[u8]) -> String {
        blake3_hex(data)
    }

    /// Return the recorded BLAKE3 output hash for `(capability, operation)`,
    /// or `None` if no recording exists for that pair.
    pub fn recorded_hash(&self, capability: &CapabilityId, operation: &str) -> Option<String> {
        self.recordings
            .iter()
            .find(|(cap, op, _, _)| cap == capability && op == operation)
            .map(|(_, _, _, hash)| hash.clone())
    }

    /// Verify that `actual_response` matches the recorded output hash for
    /// `(capability, operation)`.
    ///
    /// # Errors
    ///
    /// - [`ReplayVerificationError`] if no recording exists for the pair.
    /// - [`ReplayVerificationError`] if the BLAKE3 hash of `actual_response`
    ///   differs from the recorded hash.
    pub fn verify(
        &self,
        capability: &CapabilityId,
        operation: &str,
        actual_response: &[u8],
    ) -> Result<(), ReplayVerificationError> {
        let recorded_hash = self
            .recorded_hash(capability, operation)
            .ok_or_else(|| ReplayVerificationError::missing_recording(capability, operation))?;

        let actual_hash = blake3_hex(actual_response);
        if actual_hash != recorded_hash {
            return Err(ReplayVerificationError::hash_mismatch(
                capability,
                operation,
                recorded_hash,
                actual_hash,
            ));
        }
        Ok(())
    }

    /// Verify multiple replay outputs and return all mismatches in a stable,
    /// redacted order.
    ///
    /// The returned issues are sorted only by diagnostic key/category and
    /// redacted capability/operation shape. Raw operations and payloads are
    /// never copied into diagnostics or used as visible ordering keys.
    pub fn verify_many<I, O, B>(&self, actual_responses: I) -> Vec<ReplayVerificationError>
    where
        I: IntoIterator<Item = (CapabilityId, O, B)>,
        O: AsRef<str>,
        B: AsRef<[u8]>,
    {
        let mut issues: Vec<_> = actual_responses
            .into_iter()
            .filter_map(|(capability, operation, actual_response)| {
                self.verify(&capability, operation.as_ref(), actual_response.as_ref())
                    .err()
            })
            .collect();
        issues.sort_by(|left, right| left.diagnostic_sort_key().cmp(&right.diagnostic_sort_key()));
        issues
    }

    /// Consume the engine and produce a [`Handler`] that replays recordings
    /// without hash verification.
    pub fn into_handler(self) -> ReplayHandler {
        self.build_handler(false)
    }

    /// Consume the engine and produce a [`Handler`] that replays recordings
    /// AND verifies each response against its recorded BLAKE3 output hash.
    ///
    /// If the response bytes do not match the recorded hash, the handler
    /// returns a `HostError` instead of the response.
    pub fn into_verifying_handler(self) -> ReplayHandler {
        self.build_handler(true)
    }

    fn build_handler(self, verify: bool) -> ReplayHandler {
        let mut map: HashMap<(String, String), (Vec<u8>, String)> = HashMap::new();
        let mut seen = std::collections::HashSet::new();
        let mut caps = Vec::new();

        for (cap, op, resp, hash) in self.recordings {
            let cap_str = cap.as_str().to_string();
            if seen.insert(cap_str.clone()) {
                caps.push(CapabilityId::new(cap_str.clone()));
            }
            map.insert((cap_str, op), (resp, hash));
        }

        if caps.is_empty() {
            caps.push(CapabilityId::new("replay.unregistered"));
        }

        ReplayHandler {
            map: Arc::new(map),
            caps,
            verify,
        }
    }
}

impl Default for ReplayEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── ReplayHandler ─────────────────────────────────────────────────────────

/// Handler produced by [`ReplayEngine::into_handler`] or
/// [`ReplayEngine::into_verifying_handler`].
///
/// Replays pre-recorded capability responses deterministically.
/// When `verify = true`, each replayed response is checked against its
/// recorded BLAKE3 output hash and a `HostError` is returned on mismatch.
pub struct ReplayHandler {
    /// (cap_str, operation) → (response, recorded_hash)
    map: Arc<ReplayResponseMap>,
    caps: Vec<CapabilityId>,
    /// When `true`, responses are verified against their recorded hashes.
    verify: bool,
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
            Some((resp, recorded_hash)) => {
                if self.verify {
                    let actual_hash = blake3_hex(resp);
                    if &actual_hash != recorded_hash {
                        let err = ReplayVerificationError::hash_mismatch(
                            capability,
                            operation,
                            recorded_hash.clone(),
                            actual_hash,
                        );
                        return Err(HostError::ManifestMismatch(err.to_string()));
                    }
                }
                Ok(resp.clone())
            }
            None => Err(HostError::HandlerUnavailable(
                ReplayVerificationError::missing_recording(capability, operation).to_string(),
            )),
        }
    }
}

// ── TamperTestHandler ─────────────────────────────────────────────────────

/// Test helper that returns responses different from recorded ones.
///
/// Used in tests to prove that replay hash verification detects tampering.
/// Not intended for production use.
pub struct TamperTestHandler {
    caps: Vec<CapabilityId>,
    tampered_response: Vec<u8>,
}

impl TamperTestHandler {
    /// Create a handler that always returns `tampered_response` regardless
    /// of capability or operation.
    pub fn new(caps: Vec<CapabilityId>, tampered_response: Vec<u8>) -> Self {
        TamperTestHandler {
            caps,
            tampered_response,
        }
    }
}

impl Handler for TamperTestHandler {
    fn name(&self) -> &str {
        "TamperTestHandler"
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
        Ok(self.tampered_response.clone())
    }
}
