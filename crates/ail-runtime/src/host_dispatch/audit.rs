// ── ail-runtime::host_dispatch::audit ─────────────────────────────────────

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::audit::{AuditEvent, AuditLog};
use crate::host_dispatch::trace::TraceContext;
use crate::profile::CapabilityId;

#[derive(Clone)]
pub(crate) struct CapabilityAuditContext {
    pub(crate) start: Instant,
    pub(crate) timestamp: u64,
    pub(crate) profile: Option<String>,
    pub(crate) module: Option<String>,
    pub(crate) input_hash: Option<String>,
    pub(crate) trace_id: Option<String>,
    pub(crate) verification_report_hash: Option<String>,
    pub(crate) trace_context: Option<TraceContext>,
    /// Generic failure category set by the runtime or handler on denial.
    ///
    /// Defaults to `None`.  Use `push_denied` for runtime-controlled denial
    /// categories, or set this field on a clone before calling `push` when the
    /// handler returned a categorized denial.  The category MUST NOT contain
    /// secret IDs, vault paths, raw payloads, or any sensitive data.
    pub(crate) denial_category: Option<String>,
}

impl CapabilityAuditContext {
    /// Append a [`AuditEvent::CapabilityCallExecuted`] event to `audit_log`.
    ///
    /// The `denial_category` field of this context is forwarded to the event.
    pub(crate) fn with_denial_category(&self, category: &'static str) -> Self {
        let mut audit = self.clone();
        audit.denial_category = crate::audit::denial_category(category);
        audit
    }

    pub(crate) fn push_denied(
        &self,
        audit_log: &Arc<Mutex<AuditLog>>,
        capability: CapabilityId,
        operation: String,
        handler_name: String,
        category: &'static str,
    ) {
        self.with_denial_category(category).push(
            audit_log,
            capability,
            operation,
            handler_name,
            false,
            None,
        );
    }

    pub(crate) fn push(
        &self,
        audit_log: &Arc<Mutex<AuditLog>>,
        capability: CapabilityId,
        operation: String,
        handler_name: String,
        succeeded: bool,
        output_hash: Option<String>,
    ) {
        audit_log
            .lock()
            .expect("audit_log lock")
            .push(AuditEvent::CapabilityCallExecuted {
                capability,
                operation,
                handler_name,
                succeeded,
                duration_us: self.start.elapsed().as_micros() as u64,
                timestamp: self.timestamp,
                profile: self.profile.clone(),
                module: self.module.clone(),
                function: None,
                input_hash: self.input_hash.clone(),
                output_hash,
                trace_id: self.trace_id.clone(),
                verification_report_hash: self.verification_report_hash.clone(),
                trace_context: self.trace_context.clone(),
                denial_category: self.denial_category.clone(),
            });
    }
}
