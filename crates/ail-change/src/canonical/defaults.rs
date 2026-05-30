use super::*;

// ── materialize_defaults ──────────────────────────────────────────────────

/// Apply safe, mechanical defaults to an op's args in place.
///
/// Defaults applied:
/// - `create_function` / `create_type` without `visibility` → `"private"`.
/// - `create_type` without `derive` → `"none"`.
///
/// Rules (from spec):
/// 1. Defaults must be safe (never public/unsafe/assumed).
/// 2. Defaults must be mechanical (no semantic ambiguity).
/// 3. Defaults must not grant permissions or expose APIs.
pub(super) fn materialize_defaults(kind: &ChangeSetOp, verb: &str, args: &mut OpArgs) {
    if kind == &ChangeSetOp::Create {
        if matches!(verb, "create_function" | "create_type") {
            args.entry("visibility".to_string())
                .or_insert_with(|| "private".to_string());
        }
        if verb == "create_type" {
            args.entry("derive".to_string())
                .or_insert_with(|| "none".to_string());
        }
    }
}
