// ── replay_tests.rs ──────────────────────────────────────────────────────
//
// TDD tests for ail-runtime deterministic handlers + replay engine (G29).
// Written BEFORE implementation — RED phase.

use ail_runtime::handler::Handler;
use ail_runtime::profile::CapabilityId;
use ail_runtime::replay::{
    FakePayment, FixedClock, InMemoryDb, RecordedHttp, ReplayEngine, SeededRandom,
};

// ── FixedClock ────────────────────────────────────────────────────────────

#[test]
fn fixed_clock_returns_configured_timestamp() {
    let clock = FixedClock::new(1_700_000_000_000); // ms timestamp
    let cap = CapabilityId::new("clock.now");
    let result = clock.handle(&cap, "now", &[]);
    let bytes = result.expect("FixedClock must succeed");
    // Response is 8 bytes: little-endian u64 timestamp
    assert_eq!(bytes.len(), 8);
    let ts = u64::from_le_bytes(bytes.try_into().unwrap());
    assert_eq!(ts, 1_700_000_000_000);
}

#[test]
fn fixed_clock_declares_clock_now_capability() {
    let clock = FixedClock::new(0);
    let caps = clock.capabilities();
    assert!(
        caps.iter().any(|c| c.as_str() == "clock.now"),
        "FixedClock must declare clock.now capability"
    );
}

#[test]
fn fixed_clock_name_is_fixed_clock() {
    let clock = FixedClock::new(42);
    assert_eq!(clock.name(), "FixedClock");
}

#[test]
fn fixed_clock_now_op_returns_timestamp() {
    // Explicit guard: the "now" operation must remain valid.
    let clock = FixedClock::new(1_700_000_000_000);
    let cap = CapabilityId::new("clock.now");
    let bytes = clock
        .handle(&cap, "now", &[])
        .expect("FixedClock 'now' must succeed");
    let ts = u64::from_le_bytes(bytes.try_into().unwrap());
    assert_eq!(ts, 1_700_000_000_000);
}

#[test]
fn fixed_clock_unknown_op_returns_error() {
    // Any operation other than "now" must be rejected.
    let clock = FixedClock::new(42);
    let cap = CapabilityId::new("clock.now");
    let result = clock.handle(&cap, "tick", &[]);
    assert!(result.is_err(), "unknown operation must return error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("unknown FixedClock operation") && err.contains("tick"),
        "error must name the unknown operation: got {err}"
    );
}

// ── SeededRandom ──────────────────────────────────────────────────────────

#[test]
fn seeded_random_is_deterministic_with_same_seed() {
    let r1 = SeededRandom::new(12345);
    let r2 = SeededRandom::new(12345);
    let cap = CapabilityId::new("random.next_u64");
    let a = r1.handle(&cap, "next_u64", &[]).expect("must succeed");
    let b = r2.handle(&cap, "next_u64", &[]).expect("must succeed");
    assert_eq!(a, b, "same seed must produce same first value");
}

#[test]
fn seeded_random_advances_state_on_each_call() {
    let r = SeededRandom::new(999);
    let cap = CapabilityId::new("random.next_u64");
    let a = r.handle(&cap, "next_u64", &[]).expect("first call");
    let b = r.handle(&cap, "next_u64", &[]).expect("second call");
    assert_ne!(a, b, "consecutive calls must produce different values");
}

#[test]
fn seeded_random_declares_random_capability() {
    let r = SeededRandom::new(1);
    let caps = r.capabilities();
    assert!(
        caps.iter().any(|c| c.as_str() == "random.next_u64"),
        "SeededRandom must declare random.next_u64"
    );
}

// ── RecordedHttp ──────────────────────────────────────────────────────────

#[test]
fn recorded_http_returns_recorded_response() {
    let mut h = RecordedHttp::new();
    h.record("GET:https://api.example.com/prices", b"[100, 200]".to_vec());
    let cap = CapabilityId::new("http.call:PriceService");
    let result = h.handle(&cap, "GET:https://api.example.com/prices", b"");
    let bytes = result.expect("recorded response must be returned");
    assert_eq!(bytes, b"[100, 200]");
}

#[test]
fn recorded_http_returns_error_for_unknown_operation() {
    let h = RecordedHttp::new();
    let cap = CapabilityId::new("http.call:PriceService");
    let result = h.handle(&cap, "GET:https://unknown.example.com", b"");
    assert!(result.is_err(), "unknown operation must return error");
}

#[test]
fn recorded_http_declares_http_call_capability() {
    let h = RecordedHttp::new();
    let caps = h.capabilities();
    assert!(
        caps.iter().any(|c| c.as_str().starts_with("http.call")),
        "RecordedHttp must declare http.call capability"
    );
}

// ── InMemoryDb ────────────────────────────────────────────────────────────

#[test]
fn in_memory_db_stores_and_retrieves_record() {
    let db = InMemoryDb::new();
    db.insert("Cart:42", b"{\"items\":[]}".to_vec());
    let cap = CapabilityId::new("database.read:Cart");
    let result = db.handle(&cap, "read:Cart:42", b"");
    let bytes = result.expect("stored record must be retrievable");
    assert_eq!(bytes, b"{\"items\":[]}");
}

#[test]
fn in_memory_db_returns_empty_for_missing_key() {
    let db = InMemoryDb::new();
    let cap = CapabilityId::new("database.read:Cart");
    let result = db.handle(&cap, "read:Cart:99", b"");
    let bytes = result.expect("missing key returns empty bytes");
    assert!(bytes.is_empty(), "missing record returns empty bytes");
}

#[test]
fn in_memory_db_declares_database_capabilities() {
    let db = InMemoryDb::new();
    let caps = db.capabilities();
    assert!(
        caps.iter().any(|c| c.as_str().starts_with("database.read")),
        "InMemoryDb must declare database.read capability"
    );
    assert!(
        caps.iter()
            .any(|c| c.as_str().starts_with("database.write")),
        "InMemoryDb must declare database.write capability"
    );
}

#[test]
fn in_memory_db_write_operation_stores_payload() {
    let db = InMemoryDb::new();
    let cap_write = CapabilityId::new("database.write:Order");
    let payload = b"Order:1:{\"total\":100}";
    // write operation: op is "write:<key>", payload is the value
    db.handle(&cap_write, "write:Order:1", payload)
        .expect("write must succeed");

    let cap_read = CapabilityId::new("database.read:Order");
    let bytes = db
        .handle(&cap_read, "read:Order:1", b"")
        .expect("read after write must succeed");
    assert_eq!(bytes, payload);
}

// ── FakePayment ───────────────────────────────────────────────────────────

#[test]
fn fake_payment_succeeds_when_configured() {
    let fp = FakePayment::new(true, b"receipt-ok".to_vec());
    let cap = CapabilityId::new("payment.charge:PaymentProvider");
    let result = fp.handle(&cap, "charge", b"amount=100");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), b"receipt-ok");
}

#[test]
fn fake_payment_fails_when_configured() {
    let fp = FakePayment::new(false, b"".to_vec());
    let cap = CapabilityId::new("payment.charge:PaymentProvider");
    let result = fp.handle(&cap, "charge", b"amount=100");
    assert!(
        result.is_err(),
        "FakePayment configured to fail must return Err"
    );
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("PaymentDeclined") || err_str.contains("declined"),
        "error message must indicate payment declined: got {err:?}"
    );
}

#[test]
fn fake_payment_declares_payment_capability() {
    let fp = FakePayment::new(true, vec![]);
    let caps = fp.capabilities();
    assert!(
        caps.iter()
            .any(|c| c.as_str().starts_with("payment.charge")),
        "FakePayment must declare payment.charge capability"
    );
}

// ── ReplayEngine ──────────────────────────────────────────────────────────

#[test]
fn replay_engine_records_and_replays_response() {
    let mut engine = ReplayEngine::new();
    let cap = CapabilityId::new("database.read:Cart");
    engine.record(cap.clone(), "read:Cart:42", b"cart-data".to_vec());

    let handler = engine.into_handler();
    let result = handler.handle(&cap, "read:Cart:42", b"");
    let bytes = result.expect("replay must return recorded response");
    assert_eq!(bytes, b"cart-data");
}

#[test]
fn replay_engine_returns_error_for_unrecorded_call() {
    let engine = ReplayEngine::new();
    let handler = engine.into_handler();
    let cap = CapabilityId::new("database.read:Cart");
    let result = handler.handle(&cap, "read:Cart:99", b"");
    assert!(result.is_err(), "unrecorded call must return error");
}

#[test]
fn replay_engine_replays_same_response_multiple_times() {
    let mut engine = ReplayEngine::new();
    let cap = CapabilityId::new("http.call:PriceService");
    engine.record(cap.clone(), "GET:prices", b"[100]".to_vec());

    let handler = engine.into_handler();
    let r1 = handler
        .handle(&cap, "GET:prices", b"")
        .expect("first replay");
    let r2 = handler
        .handle(&cap, "GET:prices", b"")
        .expect("second replay");
    assert_eq!(r1, r2, "same recording must replay identically");
}
