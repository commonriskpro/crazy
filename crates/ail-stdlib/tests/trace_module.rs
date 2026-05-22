use ail_stdlib::trace::{Metric, Span, SpanId, SpanStatus, TraceId};

#[test]
fn trace_id_to_hex() {
    let id = TraceId::new(0, 0);
    assert_eq!(id.to_hex(), "00000000000000000000000000000000");
}

#[test]
fn trace_id_to_hex_nonzero() {
    let id = TraceId::new(0xDEADBEEF_CAFEBABE, 0x1234567890ABCDEF);
    assert_eq!(id.to_hex().len(), 32);
}

#[test]
fn span_id_to_hex() {
    let id = SpanId::new(0xABCD1234_EFFF0000);
    assert_eq!(id.to_hex().len(), 16);
}

#[test]
fn span_new_defaults() {
    let span = Span::new(TraceId::new(1, 2), SpanId::new(3), "test-op", 1000);
    assert_eq!(span.name, "test-op");
    assert_eq!(span.status, SpanStatus::Unset);
    assert!(span.parent_id.is_none());
    assert!(span.end_nanos.is_none());
    assert!(span.duration_nanos().is_none());
}

#[test]
fn span_with_parent() {
    let parent = SpanId::new(99);
    let span = Span::new(TraceId::new(1, 2), SpanId::new(10), "child", 0).with_parent(parent);
    assert_eq!(span.parent_id, Some(parent));
}

#[test]
fn span_set_attribute() {
    let mut span = Span::new(TraceId::new(1, 0), SpanId::new(1), "op", 0);
    span.set_attribute("http.method", "GET");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes[0], ("http.method".into(), "GET".into()));
}

#[test]
fn span_finish() {
    let mut span = Span::new(TraceId::new(1, 0), SpanId::new(1), "op", 1000);
    span.finish(3000, SpanStatus::Ok);
    assert_eq!(span.status, SpanStatus::Ok);
    assert_eq!(span.duration_nanos(), Some(2000));
}

#[test]
fn metric_new() {
    let m = Metric::new("requests.total", 42.0);
    assert_eq!(m.name, "requests.total");
    assert_eq!(m.value, 42.0);
    assert!(m.unit.is_none());
    assert!(m.labels.is_empty());
}

#[test]
fn metric_with_unit_and_label() {
    let m = Metric::new("latency", 150.0)
        .with_unit("ms")
        .with_label("endpoint", "/api/v1");
    assert_eq!(m.unit, Some("ms".into()));
    assert_eq!(m.labels.len(), 1);
    assert_eq!(m.labels[0], ("endpoint".into(), "/api/v1".into()));
}
