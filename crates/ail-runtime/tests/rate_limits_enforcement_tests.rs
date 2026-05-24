// ── ail-runtime::rate_limits_enforcement_tests ────────────────────────────
//
// Integration tests for ResourceLimits enforcement:
//   rate_limits, concurrency_limit, recursion_stack_limit.
//
// Design principles:
//   - All rate-limit tests use an injectable clock (Arc<AtomicU64>) so windows
//     are advanced by writing to the atomic, never by sleeping.  This makes
//     every test fully deterministic and instantaneous.
//   - Concurrency and depth tests use limit=0 (deny-all) and limit=1 (allow-
//     one) cases, which are trivially deterministic for serial execution.
//   - Negative tests prove that limits REJECT calls; positive tests prove that
//     limits DO NOT over-reject valid calls.
//
// Scenarios:
//   RLE1 — global rate limit 2/s: calls 1+2 succeed, call 3 is denied
//   RLE2 — per-capability rate limit applies only to the matching capability
//   RLE3 — rate limit window resets after the clock advances ≥ 1 second
//   RLE4 — max_calls_per_second=0 denies the very first call
//   RLE5 — no rate_limits configured: calls proceed without restriction
//   CC1  — concurrency_limit=0 denies all capability calls
//   CC2  — concurrency_limit=1 allows calls (serial execution never exceeds 1)
//   RS1  — recursion_stack_limit=0 denies all capability calls
//   RS2  — recursion_stack_limit=1 allows calls (serial execution depth = 1)

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ail_runtime::{
    CapabilityGrant, CapabilityId, CapabilityManifest, Handler, HostError, HostResult, RateLimit,
    ResourceLimits, RuntimeHost, RuntimeProfile, blake3_hex_of,
};

// ── FakeClock ─────────────────────────────────────────────────────────────

/// A monotonic nanosecond counter backed by an `AtomicU64`.
///
/// Allows tests to advance the clock by an exact amount without sleeping.
struct FakeClock(Arc<AtomicU64>);

impl FakeClock {
    fn new(start_nanos: u64) -> Self {
        FakeClock(Arc::new(AtomicU64::new(start_nanos)))
    }

    /// Advance the clock by `nanos` nanoseconds.
    fn advance(&self, nanos: u64) {
        self.0.fetch_add(nanos, Ordering::SeqCst);
    }

    /// Return a callable suitable for `RuntimeHost::with_clock_fn`.
    fn as_fn(&self) -> Arc<dyn Fn() -> u64 + Send + Sync> {
        let inner = self.0.clone();
        Arc::new(move || inner.load(Ordering::SeqCst))
    }
}

// ── EchoHandler ──────────────────────────────────────────────────────────

/// Minimal handler: accepts any call, returns 8 zero bytes.
struct EchoHandler {
    cap: CapabilityId,
}

impl Handler for EchoHandler {
    fn name(&self) -> &str {
        "echo"
    }
    fn capabilities(&self) -> &[CapabilityId] {
        std::slice::from_ref(&self.cap)
    }
    fn handle(&self, _: &CapabilityId, _: &str, _: &[u8]) -> HostResult<Vec<u8>> {
        Ok(vec![0u8; 8])
    }
}

// ── test helpers ─────────────────────────────────────────────────────────

/// Minimal valid WASM binary (magic + version, no sections).
fn wasm_minimal() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

/// Build a `RuntimeHost` that grants `cap`, registers an `EchoHandler` for
/// it, applies `limits`, and completes preflight — ready for `call_capability`.
///
/// Optionally replace the clock for rate limit tests.
fn make_host(
    cap: CapabilityId,
    limits: ResourceLimits,
    clock: Option<Arc<dyn Fn() -> u64 + Send + Sync>>,
) -> RuntimeHost {
    let wasm = wasm_minimal();
    let manifest = CapabilityManifest {
        module: "test-module".to_string(),
        requires: vec![cap.clone()],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash");
    let profile = RuntimeProfile::new(
        "test-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        vec![CapabilityGrant {
            module: "test-module".to_string(),
            capability: cap.clone(),
        }],
        limits,
    );
    let handler: Arc<dyn Handler + Send + Sync> = Arc::new(EchoHandler { cap });
    let mut host = {
        let base = RuntimeHost::new().with_handler(handler);
        match clock {
            Some(fn_) => base.with_clock_fn(fn_),
            None => base,
        }
    };
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");
    host
}

// ── RLE1: global rate limit ───────────────────────────────────────────────

/// Global rate limit of 2 calls/s: calls 1 and 2 succeed, call 3 is denied.
#[test]
fn global_rate_limit_denies_excess_call_in_same_window() {
    let cap = CapabilityId::new("io");
    let clock = FakeClock::new(1_000_000_000); // start at t = 1 s
    let limits = ResourceLimits {
        rate_limits: Some(vec![RateLimit {
            capability: None, // global
            max_calls_per_second: 2,
        }]),
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, Some(clock.as_fn()));

    // Call 1: allowed (window count → 1)
    assert!(
        host.call_capability(&cap, "op", &[]).is_ok(),
        "first call must succeed"
    );
    // Call 2: allowed (window count → 2)
    assert!(
        host.call_capability(&cap, "op", &[]).is_ok(),
        "second call must succeed"
    );
    // Call 3: denied (window count = 2 >= limit = 2)
    let result = host.call_capability(&cap, "op", &[]);
    assert!(
        matches!(result, Err(HostError::LimitExceeded(_))),
        "third call in same second must be denied, got {result:?}"
    );
}

// ── RLE2: per-capability rate limit ──────────────────────────────────────

/// A per-capability limit on "db" does NOT restrict "io".
#[test]
fn per_capability_rate_limit_does_not_affect_other_capabilities() {
    // We only have one capability in this profile, but we set a per-cap limit
    // on a different name — it must be a no-op for our capability.
    let cap = CapabilityId::new("io");
    let clock = FakeClock::new(1_000_000_000);
    let limits = ResourceLimits {
        rate_limits: Some(vec![RateLimit {
            capability: Some("db".to_string()), // limit on "db", not "io"
            max_calls_per_second: 0,            // would deny immediately if applied
        }]),
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, Some(clock.as_fn()));

    // All calls to "io" must succeed — the per-cap limit on "db" is irrelevant.
    for i in 1..=5 {
        assert!(
            host.call_capability(&cap, "op", &[]).is_ok(),
            "call {i} to 'io' must succeed (limit is on 'db')"
        );
    }
}

// ── RLE3: window reset after clock advance ────────────────────────────────

/// After the clock advances ≥ 1 second, the window resets and calls succeed.
#[test]
fn rate_limit_window_resets_after_one_second() {
    let cap = CapabilityId::new("io");
    let clock = FakeClock::new(1_000_000_000);
    let limits = ResourceLimits {
        rate_limits: Some(vec![RateLimit {
            capability: None,
            max_calls_per_second: 1,
        }]),
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, Some(clock.as_fn()));

    // First call succeeds.
    assert!(host.call_capability(&cap, "op", &[]).is_ok(), "first call");
    // Second call in same window is denied.
    assert!(
        matches!(
            host.call_capability(&cap, "op", &[]),
            Err(HostError::LimitExceeded(_))
        ),
        "second call must be denied"
    );

    // Advance the clock by exactly 1 second → window expires.
    clock.advance(1_000_000_000);

    // First call of new window must succeed again.
    assert!(
        host.call_capability(&cap, "op", &[]).is_ok(),
        "first call after window reset must succeed"
    );
    // Second call of new window is denied again.
    assert!(
        matches!(
            host.call_capability(&cap, "op", &[]),
            Err(HostError::LimitExceeded(_))
        ),
        "second call in new window must be denied"
    );
}

// ── RLE4: zero calls per second ───────────────────────────────────────────

/// max_calls_per_second=0 denies the very first call.
#[test]
fn zero_rate_limit_denies_first_call() {
    let cap = CapabilityId::new("io");
    let limits = ResourceLimits {
        rate_limits: Some(vec![RateLimit {
            capability: None,
            max_calls_per_second: 0,
        }]),
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, None);
    assert!(
        matches!(
            host.call_capability(&cap, "op", &[]),
            Err(HostError::LimitExceeded(_))
        ),
        "zero rate limit must deny first call"
    );
}

// ── RLE5: no rate limits ──────────────────────────────────────────────────

/// Without rate_limits, many calls all succeed.
#[test]
fn no_rate_limits_allows_many_calls() {
    let cap = CapabilityId::new("io");
    let limits = ResourceLimits {
        rate_limits: None,
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, None);
    for i in 1..=20 {
        assert!(
            host.call_capability(&cap, "op", &[]).is_ok(),
            "call {i} must succeed when no rate limits are configured"
        );
    }
}

// ── CC1: concurrency_limit = 0 ────────────────────────────────────────────

/// concurrency_limit=0 denies ALL capability calls immediately.
#[test]
fn concurrency_limit_zero_denies_all_calls() {
    let cap = CapabilityId::new("io");
    let limits = ResourceLimits {
        concurrency_limit: Some(0),
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, None);
    let result = host.call_capability(&cap, "op", &[]);
    assert!(
        matches!(result, Err(HostError::LimitExceeded(_))),
        "concurrency_limit=0 must deny all calls, got {result:?}"
    );
}

// ── CC2: concurrency_limit = 1 ────────────────────────────────────────────

/// concurrency_limit=1 allows serial calls (never more than 1 in-flight at
/// a time in synchronous execution; counter resets to 0 after each call).
#[test]
fn concurrency_limit_one_allows_serial_calls() {
    let cap = CapabilityId::new("io");
    let limits = ResourceLimits {
        concurrency_limit: Some(1),
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, None);
    // Serial calls must all succeed — each completes before the next starts.
    for i in 1..=5 {
        assert!(
            host.call_capability(&cap, "op", &[]).is_ok(),
            "serial call {i} must succeed with concurrency_limit=1"
        );
    }
}

// ── RS1: recursion_stack_limit = 0 ────────────────────────────────────────

/// recursion_stack_limit=0 denies ALL capability calls immediately.
#[test]
fn recursion_stack_limit_zero_denies_all_calls() {
    let cap = CapabilityId::new("io");
    let limits = ResourceLimits {
        recursion_stack_limit: Some(0),
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, None);
    let result = host.call_capability(&cap, "op", &[]);
    assert!(
        matches!(result, Err(HostError::LimitExceeded(_))),
        "recursion_stack_limit=0 must deny all calls, got {result:?}"
    );
}

// ── RS2: recursion_stack_limit = 1 ────────────────────────────────────────

/// recursion_stack_limit=1 allows serial calls (depth never exceeds 1 per
/// call for non-recursive handlers; decremented after each call completes).
#[test]
fn recursion_stack_limit_one_allows_serial_calls() {
    let cap = CapabilityId::new("io");
    let limits = ResourceLimits {
        recursion_stack_limit: Some(1),
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, None);
    for i in 1..=5 {
        assert!(
            host.call_capability(&cap, "op", &[]).is_ok(),
            "serial call {i} must succeed with recursion_stack_limit=1"
        );
    }
}

// ── combined: multiple limits enforce independently ────────────────────────

/// When both a rate limit AND a concurrency limit are set, both are enforced.
/// Setting concurrency_limit=0 with a generous rate limit still denies calls.
#[test]
fn combined_limits_enforce_independently() {
    let cap = CapabilityId::new("io");
    let limits = ResourceLimits {
        rate_limits: Some(vec![RateLimit {
            capability: None,
            max_calls_per_second: 100,
        }]),
        concurrency_limit: Some(0), // blocks everything regardless of rate
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, None);
    assert!(
        matches!(
            host.call_capability(&cap, "op", &[]),
            Err(HostError::LimitExceeded(_))
        ),
        "concurrency_limit=0 must deny even when rate limit is generous"
    );
}

// ── RLE6: duplicate global limits — no double-counting ────────────────────

/// Two global `RateLimit` entries with the same key must behave as if only
/// one entry exists: the window counter increments once per call, not twice.
///
/// With two globals of max_calls_per_second=2, a buggy double-increment
/// would exhaust the limit after only one call.  The correct behaviour
/// allows two calls in the window before denying the third.
#[test]
fn duplicate_global_rate_limits_increment_window_once_per_call() {
    let cap = CapabilityId::new("io");
    let clock = FakeClock::new(1_000_000_000);
    let limits = ResourceLimits {
        rate_limits: Some(vec![
            RateLimit {
                capability: None, // global limit — first entry
                max_calls_per_second: 2,
            },
            RateLimit {
                capability: None, // global limit — duplicate key
                max_calls_per_second: 2,
            },
        ]),
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, Some(clock.as_fn()));

    // Call 1: allowed (window count → 1, not 2)
    assert!(
        host.call_capability(&cap, "op", &[]).is_ok(),
        "first call must succeed with duplicate global limits"
    );
    // Call 2: allowed (window count → 2, not 4 / already-exceeded)
    assert!(
        host.call_capability(&cap, "op", &[]).is_ok(),
        "second call must succeed — window increments once per call, not twice"
    );
    // Call 3: denied (window count = 2 >= limit = 2)
    let result = host.call_capability(&cap, "op", &[]);
    assert!(
        matches!(result, Err(HostError::LimitExceeded(_))),
        "third call must be denied — limit is 2, got {result:?}"
    );
}

// ── audit: denied calls are recorded ─────────────────────────────────────

/// A call denied by the rate limit appends exactly one failed audit event.
#[test]
fn rate_limit_denial_appends_failed_audit_event() {
    let cap = CapabilityId::new("io");
    let clock = FakeClock::new(1_000_000_000);
    let limits = ResourceLimits {
        rate_limits: Some(vec![RateLimit {
            capability: None,
            max_calls_per_second: 1,
        }]),
        ..Default::default()
    };
    let mut host = make_host(cap.clone(), limits, Some(clock.as_fn()));

    host.call_capability(&cap, "op", &[]).expect("first call");
    let _ = host.call_capability(&cap, "op", &[]); // second: denied

    let log = host.audit_log();
    // preflight event + 2 capability call events = 3 total
    assert_eq!(log.len(), 3, "expected preflight + 2 capability events");
    // Last event must be a failed capability call (the denied one).
    let last = &log.events()[log.len() - 1];
    assert!(
        !last.is_passed(),
        "denied call audit event must not be marked as passed"
    );
}
