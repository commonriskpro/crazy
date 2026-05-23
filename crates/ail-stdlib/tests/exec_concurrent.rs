// Tests for concurrent channel exec entries via call_pure_stdlib.
//
// TDD: written BEFORE T13 implementation.
// Spec: STDLIB-EXEC-CONC-1..4
//
// Channel values are held as StdlibValue::Channel — a new variant that wraps
// Arc<Mutex<VecDeque<StdlibValue>>> and a capacity.

use ail_stdlib::exec::{StdlibValue, call_pure_stdlib};

// ── STDLIB-EXEC-CONC-1: channel_new returns a Channel handle ─────────────

#[test]
fn channel_new_returns_channel_value() {
    let result = call_pure_stdlib("std.concurrent.channel_new", &[StdlibValue::Int(4)]);
    assert!(
        matches!(result, Ok(StdlibValue::Channel(_))),
        "channel_new must return a Channel handle, got: {:?}",
        result
    );
}

// Triangulate: different capacity
#[test]
fn channel_new_with_capacity_16() {
    let result = call_pure_stdlib("std.concurrent.channel_new", &[StdlibValue::Int(16)]);
    assert!(
        matches!(result, Ok(StdlibValue::Channel(_))),
        "channel_new with capacity 16 must return a Channel handle"
    );
}

// ── STDLIB-EXEC-CONC-2: channel_send returns Ok(Unit) when not full ───────

#[test]
fn channel_send_returns_ok_unit_when_not_full() {
    let ch = call_pure_stdlib("std.concurrent.channel_new", &[StdlibValue::Int(4)])
        .expect("channel_new must succeed");
    let result = call_pure_stdlib("std.concurrent.channel_send", &[ch, StdlibValue::Int(42)]);
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Unit))))
    );
}

// Triangulate: send multiple values
#[test]
fn channel_send_multiple_values_all_ok() {
    let ch = call_pure_stdlib("std.concurrent.channel_new", &[StdlibValue::Int(4)])
        .expect("channel_new must succeed");
    for i in 0..3 {
        let result = call_pure_stdlib(
            "std.concurrent.channel_send",
            &[ch.clone(), StdlibValue::Int(i)],
        );
        assert_eq!(
            result,
            Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Unit)))),
            "send #{i} must return Ok(Unit)"
        );
    }
}

// ── STDLIB-EXEC-CONC-3: channel_recv returns Some(value) when available ───

#[test]
fn channel_recv_returns_some_value_when_item_available() {
    let ch = call_pure_stdlib("std.concurrent.channel_new", &[StdlibValue::Int(4)])
        .expect("channel_new must succeed");

    // Send a value first
    call_pure_stdlib(
        "std.concurrent.channel_send",
        &[ch.clone(), StdlibValue::Int(99)],
    )
    .expect("send must succeed");

    let result = call_pure_stdlib("std.concurrent.channel_recv", &[ch]);
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(99)))))
    );
}

// ── STDLIB-EXEC-CONC-4: channel_recv returns None on empty channel ────────

#[test]
fn channel_recv_returns_none_on_empty_channel() {
    let ch = call_pure_stdlib("std.concurrent.channel_new", &[StdlibValue::Int(4)])
        .expect("channel_new must succeed");
    let result = call_pure_stdlib("std.concurrent.channel_recv", &[ch]);
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// Triangulate: channel_len reflects current item count
#[test]
fn channel_len_reflects_send_count() {
    let ch = call_pure_stdlib("std.concurrent.channel_new", &[StdlibValue::Int(4)])
        .expect("channel_new must succeed");

    let len0 = call_pure_stdlib("std.concurrent.channel_len", std::slice::from_ref(&ch));
    assert_eq!(len0, Ok(StdlibValue::Int(0)));

    call_pure_stdlib(
        "std.concurrent.channel_send",
        &[ch.clone(), StdlibValue::Int(1)],
    )
    .expect("send must succeed");
    call_pure_stdlib(
        "std.concurrent.channel_send",
        &[ch.clone(), StdlibValue::Int(2)],
    )
    .expect("send must succeed");

    let len2 = call_pure_stdlib("std.concurrent.channel_len", &[ch]);
    assert_eq!(len2, Ok(StdlibValue::Int(2)));
}
