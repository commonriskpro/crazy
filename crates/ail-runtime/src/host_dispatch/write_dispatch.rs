// ── ail-runtime::host_dispatch::write_dispatch ────────────────────────────

use std::time::Instant;

use wasmtime::Caller;

use crate::audit::{
    DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED, DENIAL_CATEGORY_CAPABILITY_REVOKED,
    DENIAL_CATEGORY_HANDLER_NOT_BOUND, DENIAL_CATEGORY_LIMIT_CONCURRENCY,
    DENIAL_CATEGORY_LIMIT_MAX_CAPABILITY_CALLS, DENIAL_CATEGORY_LIMIT_OUTPUT_SIZE,
    DENIAL_CATEGORY_LIMIT_PAYLOAD_SIZE, DENIAL_CATEGORY_LIMIT_RATE,
    DENIAL_CATEGORY_LIMIT_RECURSION_DEPTH, DENIAL_CATEGORY_PAYLOAD_DECODE,
};
use crate::host_dispatch::audit::CapabilityAuditContext;
use crate::host_dispatch::limits::{check_rate_limits, unix_timestamp_micros};
use crate::host_dispatch::memory::{handler_payload, read_memory};
use crate::host_dispatch::state::HostState;
use crate::manifest::blake3_hex_of;
use crate::profile::CapabilityId;

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_host_call_write(
    caller: &mut Caller<'_, HostState>,
    cap_ptr: i32,
    cap_len: i32,
    op_ptr: i32,
    op_len: i32,
    args_ptr: i32,
    args_len: i32,
    out_ptr: i32,
    out_max: i32,
) -> Option<i32> {
    // Read capability name, operation name, and args bytes from WASM memory.
    let capability = String::from_utf8(read_memory(caller, cap_ptr, cap_len)?).ok()?;
    let operation = String::from_utf8(read_memory(caller, op_ptr, op_len)?).ok()?;
    let args_bytes = read_memory(caller, args_ptr, args_len.checked_mul(8)?)?;
    let cap = CapabilityId::new(capability);
    let start = Instant::now();
    let timestamp = unix_timestamp_micros();
    let input_hash = Some(blake3_hex_of(&args_bytes));

    let child_trace = caller.data().trace_context.as_ref().map(|ctx| ctx.child());
    let audit_log = caller.data().audit_log.clone();
    let log_write_text_limit = if cap.as_str() == "log.write" && operation == "write" {
        caller.data().profile.limits().payload_size_limit
    } else {
        None
    };
    let audit = CapabilityAuditContext {
        start,
        timestamp,
        profile: Some(caller.data().profile.name().to_string()),
        module: Some(caller.data().module_name.clone()),
        input_hash,
        trace_id: child_trace.as_ref().map(|tc| tc.trace_id.clone()),
        verification_report_hash: Some(
            caller.data().profile.verification_report_hash().to_string(),
        ),
        trace_context: child_trace,
        denial_category: None,
    };

    // Validate output buffer params after decoding call metadata so failures are auditable.
    if out_ptr < 0 || out_max < 0 {
        audit.push_denied(
            &audit_log,
            cap,
            operation,
            "none".to_string(),
            DENIAL_CATEGORY_PAYLOAD_DECODE,
        );
        return None;
    }

    // Grant check (module-scoped).
    {
        let state = caller.data_mut();
        if !state.profile.grants_capability(&state.module_name, &cap) {
            audit.push_denied(
                &audit_log,
                cap,
                operation,
                "none".to_string(),
                DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED,
            );
            return None;
        }
        if state
            .revocations
            .is_revoked(&state.module_name, cap.as_str(), state.profile.name())
        {
            audit.push_denied(
                &audit_log,
                cap,
                operation,
                "none".to_string(),
                DENIAL_CATEGORY_CAPABILITY_REVOKED,
            );
            return None;
        }
        if log_write_text_limit.is_none()
            && let Some(max_payload_bytes) = state.profile.limits().payload_size_limit
            && args_bytes.len() as u64 > max_payload_bytes
        {
            audit.push_denied(
                &audit_log,
                cap,
                operation,
                "none".to_string(),
                DENIAL_CATEGORY_LIMIT_PAYLOAD_SIZE,
            );
            return None;
        }
        if let Some(max_calls) = state.profile.limits().max_capability_calls
            && state.capability_calls_used >= max_calls
        {
            audit.push_denied(
                &audit_log,
                cap,
                operation,
                "none".to_string(),
                DENIAL_CATEGORY_LIMIT_MAX_CAPABILITY_CALLS,
            );
            return None;
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
            audit.push_denied(
                &audit_log,
                cap,
                operation,
                "none".to_string(),
                DENIAL_CATEGORY_LIMIT_RATE,
            );
            return None;
        }
        // Concurrency limit enforcement.
        if let Some(max_concurrent) = state.profile.limits().concurrency_limit
            && state.concurrent_calls >= max_concurrent
        {
            audit.push_denied(
                &audit_log,
                cap,
                operation,
                "none".to_string(),
                DENIAL_CATEGORY_LIMIT_CONCURRENCY,
            );
            return None;
        }
        // Recursion stack (call depth) limit enforcement.
        if let Some(max_depth) = state.profile.limits().recursion_stack_limit
            && state.call_depth >= max_depth
        {
            audit.push_denied(
                &audit_log,
                cap,
                operation,
                "none".to_string(),
                DENIAL_CATEGORY_LIMIT_RECURSION_DEPTH,
            );
            return None;
        }
    }

    // Find the matching handler.
    let handler = {
        let state = caller.data();
        state
            .handlers
            .iter()
            .find(|h| h.capabilities().contains(&cap))
            .cloned()
    };
    let Some(handler) = handler else {
        audit.push_denied(
            &audit_log,
            cap,
            operation,
            "none".to_string(),
            DENIAL_CATEGORY_HANDLER_NOT_BOUND,
        );
        return None;
    };
    // Increment AFTER handler is found: a granted-but-unbound call does NOT
    // consume a capability call slot.  This is intentional pre-existing
    // behavior (matches main verbatim) — contrast with `dispatch_host_call`,
    // which increments before handler lookup.  The regression tests in
    // dispatch_parity_tests.rs cover both.
    {
        let state = caller.data_mut();
        state.capability_calls_used += 1;
        // Concurrency and depth counters track in-flight calls; decremented at
        // every return point below.
        state.concurrent_calls += 1;
        state.call_depth += 1;
    }
    let handler_name = handler.name().to_string();

    // Dispatch. `log.write` is the narrow v0.2 Text bridge: decode the packed
    // Text argument before handing it to the handler; all other calls keep the
    // existing scalar bytes ABI.
    let Some(payload) =
        handler_payload(caller, &cap, &operation, &args_bytes, log_write_text_limit)
    else {
        {
            let state = caller.data_mut();
            state.concurrent_calls -= 1;
            state.call_depth -= 1;
        }
        audit.push_denied(
            &audit_log,
            cap,
            operation,
            handler_name,
            DENIAL_CATEGORY_PAYLOAD_DECODE,
        );
        return None;
    };
    let result = handler.handle(&cap, &operation, &payload);
    let response = match result {
        Ok(response) => response,
        Err(err) => {
            // Extract the generic audit category before discarding the error.
            // The category is opaque (no secret data) and recorded only in the
            // audit log.  The caller only sees the -1 return code.
            let denial_category = err.audit_category().map(|s| s.to_string());
            {
                let state = caller.data_mut();
                state.concurrent_calls -= 1;
                state.call_depth -= 1;
            }
            // Clone the context so we can attach the category without mutating
            // the shared `audit` context used by other push sites.
            let mut audit_err = audit.clone();
            audit_err.denial_category = denial_category;
            audit_err.push(&audit_log, cap, operation, handler_name, false, None);
            return None;
        }
    };

    if let Some(max_output_bytes) = caller.data().profile.limits().output_size_limit
        && response.len() as u64 > max_output_bytes
    {
        {
            let state = caller.data_mut();
            state.concurrent_calls -= 1;
            state.call_depth -= 1;
        }
        audit.push_denied(
            &audit_log,
            cap,
            operation,
            handler_name,
            DENIAL_CATEGORY_LIMIT_OUTPUT_SIZE,
        );
        return None;
    }

    // Bounds-check: response must fit in the out buffer.
    if response.len() > out_max as usize {
        {
            let state = caller.data_mut();
            state.concurrent_calls -= 1;
            state.call_depth -= 1;
        }
        audit.push_denied(
            &audit_log,
            cap,
            operation,
            handler_name,
            DENIAL_CATEGORY_LIMIT_OUTPUT_SIZE,
        );
        return None;
    }

    // Write response bytes to WASM memory at out_ptr.
    let memory = match caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
    {
        Some(memory) => memory,
        None => {
            {
                let state = caller.data_mut();
                state.concurrent_calls -= 1;
                state.call_depth -= 1;
            }
            audit.push_denied(
                &audit_log,
                cap,
                operation,
                handler_name,
                DENIAL_CATEGORY_PAYLOAD_DECODE,
            );
            return None;
        }
    };
    if memory
        .write(&mut *caller, out_ptr as usize, &response)
        .is_err()
    {
        {
            let state = caller.data_mut();
            state.concurrent_calls -= 1;
            state.call_depth -= 1;
        }
        audit.push_denied(
            &audit_log,
            cap,
            operation,
            handler_name,
            DENIAL_CATEGORY_PAYLOAD_DECODE,
        );
        return None;
    }

    let output_hash = Some(blake3_hex_of(response.as_slice()));
    {
        let state = caller.data_mut();
        state.concurrent_calls -= 1;
        state.call_depth -= 1;
    }
    audit.push(&audit_log, cap, operation, handler_name, true, output_hash);

    Some(response.len() as i32)
}
