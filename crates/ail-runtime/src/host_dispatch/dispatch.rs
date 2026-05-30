// ── ail-runtime::host_dispatch::dispatch ──────────────────────────────────

use std::time::Instant;

use wasmtime::Caller;

use crate::abi::HostError;
use crate::audit::AuditEvent;
use crate::host_dispatch::limits::{check_rate_limits, unix_timestamp_micros};
use crate::host_dispatch::memory::{handler_payload, read_memory};
use crate::host_dispatch::state::HostState;
use crate::manifest::blake3_hex_of;
use crate::profile::CapabilityId;

pub(crate) fn dispatch_host_call(
    caller: &mut Caller<'_, HostState>,
    cap_ptr: i32,
    cap_len: i32,
    op_ptr: i32,
    op_len: i32,
    args_ptr: i32,
    args_len: i32,
) -> Option<i64> {
    let capability = String::from_utf8(read_memory(caller, cap_ptr, cap_len)?).ok()?;
    let operation = String::from_utf8(read_memory(caller, op_ptr, op_len)?).ok()?;
    let args_bytes = read_memory(caller, args_ptr, args_len.checked_mul(8)?)?;
    let cap = CapabilityId::new(capability);
    let start = Instant::now();
    let timestamp = unix_timestamp_micros();
    let input_hash = Some(blake3_hex_of(&args_bytes));

    // Derive a child span from the active trace context (if any).
    let child_trace = caller.data().trace_context.as_ref().map(|ctx| ctx.child());

    // Snapshot context needed for audit events.
    let module_name = caller.data().module_name.clone();
    let profile_name = Some(caller.data().profile.name().to_string());
    let vr_hash = Some(caller.data().profile.verification_report_hash().to_string());
    let trace_id = child_trace.as_ref().map(|tc| tc.trace_id.clone());
    let log_write_text_limit = if cap.as_str() == "log.write" && operation == "write" {
        caller.data().profile.limits().payload_size_limit
    } else {
        None
    };

    let handler = {
        let state = caller.data_mut();
        // Grant check (module-scoped).
        if !state.profile.grants_capability(&state.module_name, &cap) {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        if state
            .revocations
            .is_revoked(&state.module_name, cap.as_str(), state.profile.name())
        {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        if log_write_text_limit.is_none()
            && let Some(max_payload_bytes) = state.profile.limits().payload_size_limit
            && args_bytes.len() as u64 > max_payload_bytes
        {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        if let Some(max_calls) = state.profile.limits().max_capability_calls
            && state.capability_calls_used >= max_calls
        {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        // Rate limit enforcement.
        let clock_fn = state.clock_fn.clone();
        let rate_limits_vec = state
            .profile
            .limits()
            .rate_limits
            .clone()
            .unwrap_or_default();
        if !check_rate_limits(
            &rate_limits_vec,
            &clock_fn,
            &mut state.rate_limit_windows,
            &cap,
        ) {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        // Concurrency limit enforcement.
        if let Some(max_concurrent) = state.profile.limits().concurrency_limit
            && state.concurrent_calls >= max_concurrent
        {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        // Recursion stack (call depth) limit enforcement.
        if let Some(max_depth) = state.profile.limits().recursion_stack_limit
            && state.call_depth >= max_depth
        {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        // Increment BEFORE handler lookup: a granted-but-unbound call still
        // consumes a capability call slot.  This is intentional pre-existing
        // behavior (matches main verbatim) — contrast with
        // `dispatch_host_call_write`, which increments only after a handler is
        // found.  The regression tests in dispatch_parity_tests.rs cover both.
        state.capability_calls_used += 1;
        // Concurrency and depth counters track in-flight calls; decremented at
        // every return point below.
        state.concurrent_calls += 1;
        state.call_depth += 1;
        state
            .handlers
            .iter()
            .find(|h| h.capabilities().contains(&cap))
            .cloned()
    };

    let Some(handler) = handler else {
        let state = caller.data_mut();
        state.concurrent_calls -= 1;
        state.call_depth -= 1;
        state
            .audit_log
            .lock()
            .expect("audit_log lock")
            .push(AuditEvent::CapabilityCallExecuted {
                capability: cap,
                operation,
                handler_name: "none".to_string(),
                succeeded: false,
                duration_us: start.elapsed().as_micros() as u64,
                timestamp,
                profile: profile_name,
                module: Some(module_name),
                function: None,
                input_hash,
                output_hash: None,
                trace_id,
                verification_report_hash: vr_hash,
                trace_context: child_trace,
                denial_category: None,
            });
        return Some(-1);
    };

    let handler_name = handler.name().to_string();
    let Some(payload) =
        handler_payload(caller, &cap, &operation, &args_bytes, log_write_text_limit)
    else {
        let state = caller.data_mut();
        state.concurrent_calls -= 1;
        state.call_depth -= 1;
        state
            .audit_log
            .lock()
            .expect("audit_log lock")
            .push(AuditEvent::CapabilityCallExecuted {
                capability: cap,
                operation,
                handler_name,
                succeeded: false,
                duration_us: start.elapsed().as_micros() as u64,
                timestamp,
                profile: profile_name,
                module: Some(module_name),
                function: None,
                input_hash,
                output_hash: None,
                trace_id,
                verification_report_hash: vr_hash,
                trace_context: child_trace,
                denial_category: None,
            });
        return Some(-1);
    };

    let result = match handler.handle(&cap, &operation, &payload) {
        Ok(bytes) => {
            if let Some(max_output_bytes) = caller.data().profile.limits().output_size_limit
                && bytes.len() as u64 > max_output_bytes
            {
                Err(HostError::LimitExceeded(format!(
                    "output_size_limit exceeded: limit={max_output_bytes}"
                )))
            } else {
                Ok(bytes)
            }
        }
        Err(err) => Err(err),
    };
    // Extract the generic audit category before the result is consumed.
    // The category is opaque (no secret data) and recorded only in the audit
    // log.  The caller only sees the -1 return code — no category is leaked.
    let denial_category = result
        .as_ref()
        .err()
        .and_then(|e| e.audit_category())
        .map(|s| s.to_string());
    let succeeded = result.is_ok();
    let output_hash = result
        .as_ref()
        .ok()
        .map(|bytes| blake3_hex_of(bytes.as_slice()));
    {
        let state = caller.data_mut();
        state.concurrent_calls -= 1;
        state.call_depth -= 1;
        state
            .audit_log
            .lock()
            .expect("audit_log lock")
            .push(AuditEvent::CapabilityCallExecuted {
                capability: cap,
                operation,
                handler_name,
                succeeded,
                duration_us: start.elapsed().as_micros() as u64,
                timestamp,
                profile: profile_name,
                module: Some(module_name),
                function: None,
                input_hash,
                output_hash,
                trace_id,
                verification_report_hash: vr_hash,
                trace_context: child_trace,
                denial_category,
            });
    }

    match result {
        Ok(bytes) if bytes.len() >= 8 => {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&bytes[..8]);
            Some(i64::from_le_bytes(buf))
        }
        Ok(_) => Some(0),
        Err(_) => Some(-1),
    }
}
