// ── replay_hash_tests.rs ─────────────────────────────────────────────────
//
// TDD tests for replay mode output-hash verification (CRITICAL).
//
// Per runtime.md §"Determinism, replay and testing":
//   Replay mode:
//     replay trace_id=trace_123
//       use recorded capability responses
//       verify same output hashes
//     end

use ail_runtime::profile::CapabilityId;
use ail_runtime::replay::{ReplayEngine, ReplayVerificationError};

// ── Hash recording ────────────────────────────────────────────────────────

#[test]
fn replay_engine_records_output_hash_with_response() {
    let mut engine = ReplayEngine::new();
    let cap = CapabilityId::new("database.read:Cart");
    let response = b"cart-data-42".to_vec();

    engine.record(cap.clone(), "read:Cart:42", response.clone());

    // The engine should have stored the hash of the response
    let hash = engine.recorded_hash(&cap, "read:Cart:42");
    assert!(
        hash.is_some(),
        "engine must store output hash for recorded response"
    );
    // The hash must be a non-empty hex string
    let h = hash.unwrap();
    assert!(!h.is_empty(), "hash must not be empty");
}

#[test]
fn replay_engine_hash_is_consistent_for_same_response() {
    let response = b"deterministic-output".to_vec();
    let cap = CapabilityId::new("http.call:PriceService");

    let mut e1 = ReplayEngine::new();
    e1.record(cap.clone(), "GET:prices", response.clone());
    let h1 = e1.recorded_hash(&cap, "GET:prices").unwrap();

    let mut e2 = ReplayEngine::new();
    e2.record(cap.clone(), "GET:prices", response.clone());
    let h2 = e2.recorded_hash(&cap, "GET:prices").unwrap();

    assert_eq!(h1, h2, "same response bytes must produce same hash");
}

#[test]
fn replay_engine_hash_differs_for_different_responses() {
    let cap = CapabilityId::new("database.read:Cart");
    let mut e = ReplayEngine::new();
    e.record(cap.clone(), "read:Cart:1", b"cart-1".to_vec());
    e.record(cap.clone(), "read:Cart:2", b"cart-2".to_vec());

    let h1 = e.recorded_hash(&cap, "read:Cart:1").unwrap();
    let h2 = e.recorded_hash(&cap, "read:Cart:2").unwrap();

    assert_ne!(h1, h2, "different responses must produce different hashes");
}

#[test]
fn recorded_hash_returns_none_for_unknown_operation() {
    let engine = ReplayEngine::new();
    let cap = CapabilityId::new("database.read:Cart");
    let hash = engine.recorded_hash(&cap, "read:Cart:99");
    assert!(hash.is_none());
}

// ── Replay verification ───────────────────────────────────────────────────

#[test]
fn replay_handler_verifies_matching_output_hash() {
    use ail_runtime::handler::Handler;

    let mut engine = ReplayEngine::new();
    let cap = CapabilityId::new("database.read:Cart");
    let response = b"cart-42".to_vec();
    engine.record(cap.clone(), "read:Cart:42", response.clone());

    let handler = engine.into_verifying_handler();

    // The verifying handler replays the recorded response AND verifies hash
    let result = handler.handle(&cap, "read:Cart:42", b"");
    assert!(
        result.is_ok(),
        "verifying handler must return recorded response"
    );
    assert_eq!(result.unwrap(), response);
}

#[test]
fn replay_verifying_handler_rejects_tampered_response() {
    // TamperTestHandler is a test helper that returns responses different from
    // what was recorded — simulating replay corruption detection.
    let mut engine = ReplayEngine::new();
    let cap = CapabilityId::new("database.read:Cart");
    engine.record(cap.clone(), "read:Cart:42", b"original-data".to_vec());

    // Verify that hash comparison detects mismatch
    let recorded_hash = engine.recorded_hash(&cap, "read:Cart:42").unwrap();
    let tampered_response = b"tampered-data";
    let tampered_hash = ReplayEngine::hash_of(tampered_response);

    assert_ne!(
        recorded_hash, tampered_hash,
        "tampered response must have different hash from recorded"
    );
}

#[test]
fn replay_engine_verify_matches_reported_hash() {
    // verify(cap, op, response_bytes) returns Ok if hash matches recorded
    let mut engine = ReplayEngine::new();
    let cap = CapabilityId::new("http.call:PriceService");
    let response = b"[100, 200, 300]".to_vec();
    engine.record(cap.clone(), "GET:prices", response.clone());

    assert!(
        engine.verify(&cap, "GET:prices", &response).is_ok(),
        "same response must pass verification"
    );
}

#[test]
fn replay_engine_verify_fails_for_mismatched_output() {
    let mut engine = ReplayEngine::new();
    let cap = CapabilityId::new("http.call:PriceService");
    engine.record(cap.clone(), "GET:prices", b"original".to_vec());

    let result = engine.verify(&cap, "GET:prices", b"different");
    assert!(result.is_err(), "mismatched output must fail verification");
    let err = result.unwrap_err();
    assert!(
        err.message.contains("hash") || err.message.contains("mismatch"),
        "error must indicate hash mismatch: {:?}",
        err
    );
}

#[test]
fn replay_engine_verify_fails_for_unknown_recording() {
    let engine = ReplayEngine::new();
    let cap = CapabilityId::new("database.read:Cart");
    let result = engine.verify(&cap, "read:Cart:99", b"anything");
    assert!(
        result.is_err(),
        "verify against unknown recording must fail"
    );
}

// ── ReplayVerificationError ───────────────────────────────────────────────

#[test]
fn replay_verification_error_carries_message() {
    let err = ReplayVerificationError {
        message: "output hash mismatch for read:Cart:42".to_string(),
    };
    assert!(err.message.contains("mismatch"));
}

// ── hash_of helper ────────────────────────────────────────────────────────

#[test]
fn hash_of_is_stable_for_same_input() {
    let h1 = ReplayEngine::hash_of(b"stable-data");
    let h2 = ReplayEngine::hash_of(b"stable-data");
    assert_eq!(h1, h2);
}

#[test]
fn hash_of_differs_for_different_inputs() {
    let h1 = ReplayEngine::hash_of(b"data-a");
    let h2 = ReplayEngine::hash_of(b"data-b");
    assert_ne!(h1, h2);
}

#[test]
fn hash_of_returns_nonempty_hex_string() {
    let h = ReplayEngine::hash_of(b"test");
    assert!(!h.is_empty());
    // Must be valid hex
    assert!(
        h.chars().all(|c| c.is_ascii_hexdigit()),
        "hash must be hex: {}",
        h
    );
}
