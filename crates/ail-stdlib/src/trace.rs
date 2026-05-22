// ── ail-stdlib::trace ─────────────────────────────────────────────────────
//
// Tracing and span types for the AIL `std.trace` module.
//
// # Capabilities (from docs/stdlib.md)
//
// - trace.emit
// - metric.emit
//
// # Rules
//
// - logs are effects
// - runtime audit separate from app logs

// ── TraceId / SpanId ──────────────────────────────────────────────────────

/// A unique trace identifier (128-bit, represented as two u64s).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TraceId(pub u64, pub u64);

impl TraceId {
    pub fn new(hi: u64, lo: u64) -> Self {
        Self(hi, lo)
    }

    /// Format as a 32-char lowercase hex string.
    pub fn to_hex(&self) -> String {
        format!("{:016x}{:016x}", self.0, self.1)
    }
}

/// A unique span identifier (64-bit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpanId(pub u64);

impl SpanId {
    pub fn new(v: u64) -> Self {
        Self(v)
    }
    pub fn to_hex(&self) -> String {
        format!("{:016x}", self.0)
    }
}

// ── SpanStatus ────────────────────────────────────────────────────────────

/// The status of a span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanStatus {
    Unset,
    Ok,
    Error,
}

// ── Span ──────────────────────────────────────────────────────────────────

/// A distributed tracing span.
#[derive(Clone, Debug)]
pub struct Span {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub parent_id: Option<SpanId>,
    pub name: String,
    pub status: SpanStatus,
    pub attributes: Vec<(String, String)>,
    pub start_nanos: u64,
    pub end_nanos: Option<u64>,
}

impl Span {
    pub fn new(
        trace_id: TraceId,
        span_id: SpanId,
        name: impl Into<String>,
        start_nanos: u64,
    ) -> Self {
        Self {
            trace_id,
            span_id,
            parent_id: None,
            name: name.into(),
            status: SpanStatus::Unset,
            attributes: Vec::new(),
            start_nanos,
            end_nanos: None,
        }
    }

    pub fn with_parent(mut self, parent: SpanId) -> Self {
        self.parent_id = Some(parent);
        self
    }

    pub fn set_attribute(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.attributes.push((key.into(), value.into()));
    }

    pub fn finish(&mut self, end_nanos: u64, status: SpanStatus) {
        self.end_nanos = Some(end_nanos);
        self.status = status;
    }

    pub fn duration_nanos(&self) -> Option<u64> {
        self.end_nanos.map(|e| e.saturating_sub(self.start_nanos))
    }
}

// ── Metric ────────────────────────────────────────────────────────────────

/// A metric observation (name, value, optional unit).
#[derive(Clone, Debug, PartialEq)]
pub struct Metric {
    pub name: String,
    pub value: f64,
    pub unit: Option<String>,
    pub labels: Vec<(String, String)>,
}

impl Metric {
    pub fn new(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value,
            unit: None,
            labels: Vec::new(),
        }
    }

    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push((key.into(), value.into()));
        self
    }
}
