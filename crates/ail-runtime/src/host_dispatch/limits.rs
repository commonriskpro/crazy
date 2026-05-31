// ── ail-runtime::host_dispatch::limits ────────────────────────────────────

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::profile::{CapabilityId, RateLimit};

// ── Rate-limit diagnostics ───────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RateLimitScope {
    Global,
    Capability(String),
}

impl RateLimitScope {
    fn from_limit(limit: &RateLimit) -> Self {
        match &limit.capability {
            Some(capability) => RateLimitScope::Capability(capability.clone()),
            None => RateLimitScope::Global,
        }
    }

    fn applies_to(&self, cap: &CapabilityId) -> bool {
        match self {
            RateLimitScope::Global => true,
            RateLimitScope::Capability(name) => name == cap.as_str(),
        }
    }

    fn window_key(&self) -> Option<String> {
        match self {
            RateLimitScope::Global => None,
            RateLimitScope::Capability(name) => Some(name.clone()),
        }
    }

    pub(crate) fn label(&self) -> &str {
        match self {
            RateLimitScope::Global => "global",
            RateLimitScope::Capability(_) => "capability",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RateLimitRejection {
    pub(crate) scope: RateLimitScope,
    pub(crate) max_calls_per_second: u64,
    pub(crate) used_in_window: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RateLimitDecision {
    Allowed,
    Rejected(RateLimitRejection),
}

impl RateLimitDecision {
    pub(crate) fn is_allowed(&self) -> bool {
        matches!(self, RateLimitDecision::Allowed)
    }
}

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
    check_rate_limits_detailed(rate_limits, clock_fn, windows, cap).is_allowed()
}

/// Check rate limits and return a deterministic diagnostic decision.
///
/// Applicable limits are canonicalized by scope before enforcement:
/// - duplicate scopes are merged into the strictest threshold;
/// - global limits are checked before per-capability limits;
/// - per-capability scopes are checked lexicographically.
///
/// That keeps enforcement independent of profile declaration order and gives
/// operators stable limit diagnostics without embedding payloads or secrets.
pub(crate) fn check_rate_limits_detailed(
    rate_limits: &[RateLimit],
    clock_fn: &ClockFn,
    windows: &mut HashMap<Option<String>, (u64, u64)>,
    cap: &CapabilityId,
) -> RateLimitDecision {
    if rate_limits.is_empty() {
        return RateLimitDecision::Allowed;
    }

    const WINDOW_NANOS: u64 = 1_000_000_000; // 1 second
    let now = clock_fn();
    let applicable_limits = applicable_rate_limits(rate_limits, cap);

    // Pass 1: check all applicable limits without mutating state.
    for (scope, max_calls_per_second) in &applicable_limits {
        let key = scope.window_key();
        let (window_start, count) = windows.get(key).copied().unwrap_or((now, 0));
        let effective_count = if now.saturating_sub(window_start) >= WINDOW_NANOS {
            0 // window expired; effective count resets to 0
        } else {
            count
        };
        if effective_count >= *max_calls_per_second {
            return RateLimitDecision::Rejected(RateLimitRejection {
                scope: scope.clone(),
                max_calls_per_second: *max_calls_per_second,
                used_in_window: effective_count,
            });
        }
    }

    // Pass 2: update all applicable windows (only reached if all checks pass).
    // Canonicalization above already collapsed duplicate RateLimit entries
    // sharing the same key, so each scope increments exactly once per call.
    for scope in applicable_limits.keys() {
        let key = scope.window_key();
        let window = windows.entry(key).or_insert((now, 0));
        if now.saturating_sub(window.0) >= WINDOW_NANOS {
            *window = (now, 1); // start a fresh window
        } else {
            window.1 += 1;
        }
    }

    RateLimitDecision::Allowed
}

fn applicable_rate_limits(
    rate_limits: &[RateLimit],
    cap: &CapabilityId,
) -> BTreeMap<RateLimitScope, u64> {
    let mut applicable = BTreeMap::new();
    for limit in rate_limits {
        let scope = RateLimitScope::from_limit(limit);
        if !scope.applies_to(cap) {
            continue;
        }
        applicable
            .entry(scope)
            .and_modify(|max| *max = (*max).min(limit.max_calls_per_second))
            .or_insert(limit.max_calls_per_second);
    }
    applicable
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_clock() -> ClockFn {
        Arc::new(|| 1_000_000_000)
    }

    #[test]
    fn duplicate_rate_limits_use_strictest_threshold_independent_of_order() {
        let cap = CapabilityId::new("io");

        let first_order = vec![
            RateLimit {
                capability: None,
                max_calls_per_second: 3,
            },
            RateLimit {
                capability: None,
                max_calls_per_second: 1,
            },
        ];
        let second_order = vec![
            RateLimit {
                capability: None,
                max_calls_per_second: 1,
            },
            RateLimit {
                capability: None,
                max_calls_per_second: 3,
            },
        ];

        let mut first_windows = HashMap::new();
        assert!(check_rate_limits(
            &first_order,
            &fixed_clock(),
            &mut first_windows,
            &cap
        ));
        let first_decision =
            check_rate_limits_detailed(&first_order, &fixed_clock(), &mut first_windows, &cap);

        let mut second_windows = HashMap::new();
        assert!(check_rate_limits(
            &second_order,
            &fixed_clock(),
            &mut second_windows,
            &cap
        ));
        let second_decision =
            check_rate_limits_detailed(&second_order, &fixed_clock(), &mut second_windows, &cap);

        let expected = RateLimitDecision::Rejected(RateLimitRejection {
            scope: RateLimitScope::Global,
            max_calls_per_second: 1,
            used_in_window: 1,
        });
        assert_eq!(first_decision, expected);
        assert_eq!(second_decision, expected);
    }

    #[test]
    fn rejected_decision_does_not_mutate_windows() {
        let cap = CapabilityId::new("io");
        let limits = vec![RateLimit {
            capability: None,
            max_calls_per_second: 0,
        }];
        let mut windows = HashMap::new();

        let decision = check_rate_limits_detailed(&limits, &fixed_clock(), &mut windows, &cap);

        assert_eq!(
            decision,
            RateLimitDecision::Rejected(RateLimitRejection {
                scope: RateLimitScope::Global,
                max_calls_per_second: 0,
                used_in_window: 0,
            })
        );
        assert!(windows.is_empty());
    }

    #[test]
    fn global_scope_is_reported_before_capability_scope() {
        let cap = CapabilityId::new("io");
        let limits = vec![
            RateLimit {
                capability: Some("io".to_string()),
                max_calls_per_second: 0,
            },
            RateLimit {
                capability: None,
                max_calls_per_second: 0,
            },
        ];
        let mut windows = HashMap::new();

        let decision = check_rate_limits_detailed(&limits, &fixed_clock(), &mut windows, &cap);

        assert_eq!(
            decision,
            RateLimitDecision::Rejected(RateLimitRejection {
                scope: RateLimitScope::Global,
                max_calls_per_second: 0,
                used_in_window: 0,
            })
        );
    }

    #[test]
    fn non_matching_capability_limits_are_ignored() {
        let cap = CapabilityId::new("io");
        let limits = vec![RateLimit {
            capability: Some("db".to_string()),
            max_calls_per_second: 0,
        }];
        let mut windows = HashMap::new();

        assert!(check_rate_limits(
            &limits,
            &fixed_clock(),
            &mut windows,
            &cap
        ));
        assert!(windows.is_empty());
    }
}
