// ── ail-runtime::host_dispatch::trace ─────────────────────────────────────

/// Carries the W3C-compatible identifiers for a single logical trace span.
/// When a [`TraceContext`] is active on a [`RuntimeHost`] or [`RuntimeInstance`],
/// every capability call creates a **child span**: the child inherits
/// `trace_id`, gets a fresh `span_id`, and records the parent's `span_id` in
/// `parent_span_id`.  The child context is attached to the
/// [`AuditEvent::CapabilityCallExecuted`] event for correlation.
///
/// [`RuntimeHost`]: crate::host::RuntimeHost
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceContext {
    /// Globally unique identifier for the logical trace (e.g. W3C `traceparent` trace-id).
    pub trace_id: String,
    /// Unique identifier for this specific span within the trace.
    pub span_id: String,
    /// The `span_id` of the direct parent span, or `None` for root spans.
    pub parent_span_id: Option<String>,
}

impl TraceContext {
    /// Derive a child span from this context.
    ///
    /// The child inherits `trace_id`, gets a fresh monotonic `span_id`, and
    /// sets `parent_span_id` to this span's `span_id`.
    pub fn child(&self) -> TraceContext {
        TraceContext {
            trace_id: self.trace_id.clone(),
            span_id: next_span_id(),
            parent_span_id: Some(self.span_id.clone()),
        }
    }
}

/// Generate a unique span ID using a monotonic counter.
///
/// The IDs are process-unique and ordered.  They are not cryptographically
/// random — use an external tracing library (e.g. `opentelemetry`) for
/// production-grade IDs.
fn next_span_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{id:016x}")
}
