// ── ail-runtime::host_dispatch::limits ────────────────────────────────────

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::profile::{CapabilityId, RateLimit};

// ── helpers ───────────────────────────────────────────────────────────────

/// Return the current Unix timestamp in microseconds.
///
/// Used to stamp [`AuditEvent::CapabilityCallExecuted`] events.
/// Falls back to 0 if the system clock is before the Unix epoch (pathological).
pub(crate) fn unix_timestamp_micros() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

// ── Clock abstraction for rate limit windows ──────────────────────────────

/// A function that returns the current time as nanoseconds since Unix epoch.
///
/// `Arc<dyn Fn()>` instead of a trait keeps the API simple and avoids
/// object-safety constraints.  The default implementation calls
/// `SystemTime::now()`; tests inject a controllable counter instead.
pub(crate) type ClockFn = Arc<dyn Fn() -> u64 + Send + Sync>;

/// Return the default wall-clock `ClockFn` (nanoseconds since Unix epoch).
pub(crate) fn default_clock_fn() -> ClockFn {
    Arc::new(|| {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    })
}

/// Check rate limits for `cap` and update the sliding-window counters.
///
/// Uses a **fixed window** strategy: each `RateLimit` entry maintains an
/// independent `(window_start_nanos, call_count)` pair.  When the clock
/// advances ≥ 1 second past `window_start`, the window resets.
///
/// Returns `true` if the call is allowed, `false` if any applicable limit
/// would be exceeded.  Uses a two-pass approach: check ALL limits before
/// mutating ANY window so that a denial leaves the state unchanged.
///
/// `rate_limits` — the ordered list of limits from the active profile.
/// `clock_fn`    — injectable clock (nanoseconds since Unix epoch).
/// `windows`     — per-limit window state stored in `HostState`.
/// `cap`         — the capability being invoked (used for per-cap matching).
pub(crate) fn check_rate_limits(
    rate_limits: &[RateLimit],
    clock_fn: &ClockFn,
    windows: &mut HashMap<Option<String>, (u64, u64)>,
    cap: &CapabilityId,
) -> bool {
    if rate_limits.is_empty() {
        return true;
    }

    const WINDOW_NANOS: u64 = 1_000_000_000; // 1 second
    let now = clock_fn();

    // Pass 1: check all applicable limits without mutating state.
    for rl in rate_limits {
        let applies = rl.capability.is_none() || rl.capability.as_deref() == Some(cap.as_str());
        if !applies {
            continue;
        }
        let key = &rl.capability;
        let (window_start, count) = windows.get(key).copied().unwrap_or((now, 0));
        let effective_count = if now.saturating_sub(window_start) >= WINDOW_NANOS {
            0 // window expired; effective count resets to 0
        } else {
            count
        };
        if effective_count >= rl.max_calls_per_second {
            return false;
        }
    }

    // Pass 2: update all applicable windows (only reached if all checks pass).
    // Track which keys have already been incremented so that duplicate RateLimit
    // entries sharing the same key (e.g. two global `capability: None` entries)
    // do not double-count a single call.
    let mut updated_keys: HashSet<Option<String>> = HashSet::new();
    for rl in rate_limits {
        let applies = rl.capability.is_none() || rl.capability.as_deref() == Some(cap.as_str());
        if !applies {
            continue;
        }
        let key = rl.capability.clone();
        if !updated_keys.insert(key.clone()) {
            // This key was already incremented by an earlier duplicate entry.
            continue;
        }
        let window = windows.entry(key).or_insert((now, 0));
        if now.saturating_sub(window.0) >= WINDOW_NANOS {
            *window = (now, 1); // start a fresh window
        } else {
            window.1 += 1;
        }
    }

    true
}
