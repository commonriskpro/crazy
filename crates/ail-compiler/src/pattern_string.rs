// ── ail-compiler::pattern_string ─────────────────────────────────────────
//
// Canonical constructor-pattern parsing helpers shared across WASM backends.
//
// # Why this module exists
//
// `wasm_emit.rs` and `wasm_abi.rs` each previously maintained their own
// variant of the same pattern-parsing logic:
//
//   - `wasm_emit::parse_constructor_pattern` — full `(tag, Option<binding>)` parse
//   - `wasm_emit::is_unsupported_pattern_shape` — upfront rejection guard
//   - `wasm_abi::arm_payload_binding` — convenience wrapper returning only the binding
//
// The two implementations diverged on malformed input (e.g. `"Ok("` without a
// closing paren): `arm_payload_binding` returned `Some("")` while
// `parse_constructor_pattern` correctly returned `None`.
//
// This module is the single authoritative source.  Both backend modules now
// import from here.

/// Parse a variant constructor pattern string into `(tag, Option<binding>)`.
///
/// Recognises:
/// - `"None"` → `("None", None)` — tag-only, no payload binding
/// - `"Ok(x)"` → `("Ok", Some("x"))` — tag with single binding
/// - `"Some(_)"` → `("Some", Some("_"))` — tag with wildcard (callers decide
///   whether to emit a real binding for `_`)
///
/// Returns `None` for patterns that are not constructor-shaped:
/// - Integers, booleans, bare `_` wildcard
/// - Multi-binding patterns: `"Pair(a, b)"`
/// - Nested constructors: `"Ok(Some(x))"`
/// - Malformed patterns: `"Ok("` (no closing paren)
///
/// Callers that only need a boolean "is this a constructor?" check can test
/// `parse_constructor_pattern(p).is_some()`.
pub(crate) fn parse_constructor_pattern(pattern: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = pattern.trim();
    // Must start with an ASCII uppercase letter to be a constructor tag.
    let first_char = trimmed.chars().next()?;
    if !first_char.is_ascii_uppercase() {
        return None;
    }
    if let Some(open) = trimmed.find('(') {
        let tag = trimmed[..open].trim();
        let after = trimmed[open + 1..].trim();
        // Require exactly one closing paren at the end — malformed input → None.
        let after = after.strip_suffix(')')?;
        let binding = after.trim();
        // Reject multi-binding or nested patterns — not supported yet.
        if binding.contains('(') || binding.contains(',') {
            return None;
        }
        Some((tag, Some(binding)))
    } else {
        // Tag-only pattern (no payload).
        Some((trimmed, None))
    }
}

/// Extract the payload binding name from a single-binding constructor pattern.
///
/// This is a convenience wrapper around [`parse_constructor_pattern`] for
/// callers that only need the binding variable name (e.g. `collect_free_vars`
/// and `infer_expr_type`).
///
/// Differences from the full parse:
/// - Returns `None` for wildcard bindings (`"Ok(_)"`) — `_` is not a real
///   variable and should not be added to the locals/bound set.
/// - Returns `None` for tag-only patterns (`"None"`) — no binding to extract.
///
/// Examples:
/// - `"Ok(x)"` → `Some("x")`
/// - `"Some(value)"` → `Some("value")`
/// - `"None"` → `None` (tag-only)
/// - `"Some(_)"` → `None` (wildcard — not a real variable)
/// - `"_"` → `None`
/// - `"Ok("` → `None` (malformed — no closing paren)
/// - `"Ok(Some(x))"` → `None` (nested — unsupported)
/// - `"Pair(a, b)"` → `None` (multi-binding — unsupported)
pub(crate) fn arm_payload_binding(pattern: &str) -> Option<&str> {
    let (_, binding) = parse_constructor_pattern(pattern)?;
    let name = binding?;
    // Wildcard binding is not a real variable.
    if name == "_" { None } else { Some(name) }
}

/// Returns `true` when the pattern string looks like a constructor application
/// but uses syntax the WASM backend does not yet support.
///
/// Detected unsupported shapes:
/// - Nested constructors: `"Ok(Some(x))"` — payload contains `(`
/// - Multi-binding tuples: `"Pair(a, b)"` — payload contains `,`
/// - Record-field syntax: `"{field: val}"` — pattern starts with `{`
///
/// Used to distinguish "pattern we don't understand at all" from "pattern we
/// understand is a constructor but whose payload is too complex", enabling a
/// `CompileError::UnsupportedPatternSyntax` instead of a silent runtime
/// `Unreachable`.
pub(crate) fn is_unsupported_pattern_shape(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    // Record-field syntax (e.g. `{name: x}`).
    if trimmed.starts_with('{') {
        return true;
    }
    // Constructor with payload: starts uppercase, contains `(...)`.
    if trimmed
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
        && let Some(open) = trimmed.find('(')
    {
        // Strip the closing `)` if present; inspect the payload.
        let payload = trimmed[open + 1..].trim();
        let payload = payload.strip_suffix(')').unwrap_or(payload).trim();
        // Nested constructor or multi-binding payload.
        return payload.contains('(') || payload.contains(',');
    }
    false
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{arm_payload_binding, is_unsupported_pattern_shape, parse_constructor_pattern};

    // ── parse_constructor_pattern ──────────────────────────────────────────

    #[test]
    fn pcp_tag_only() {
        assert_eq!(parse_constructor_pattern("None"), Some(("None", None)));
        assert_eq!(parse_constructor_pattern("True"), Some(("True", None)));
    }

    #[test]
    fn pcp_single_binding() {
        assert_eq!(parse_constructor_pattern("Ok(x)"), Some(("Ok", Some("x"))));
        assert_eq!(
            parse_constructor_pattern("Some(value)"),
            Some(("Some", Some("value")))
        );
    }

    #[test]
    fn pcp_wildcard_binding() {
        // Wildcard is a valid binding string — parse succeeds; caller decides.
        assert_eq!(
            parse_constructor_pattern("Some(_)"),
            Some(("Some", Some("_")))
        );
    }

    #[test]
    fn pcp_trims_whitespace() {
        assert_eq!(
            parse_constructor_pattern("  Ok( x )  "),
            Some(("Ok", Some("x")))
        );
    }

    #[test]
    fn pcp_lowercase_start_returns_none() {
        assert_eq!(parse_constructor_pattern("ok(x)"), None);
        assert_eq!(parse_constructor_pattern(""), None);
        assert_eq!(parse_constructor_pattern("_"), None);
    }

    #[test]
    fn pcp_multi_binding_returns_none() {
        assert_eq!(parse_constructor_pattern("Pair(a, b)"), None);
    }

    #[test]
    fn pcp_nested_constructor_returns_none() {
        assert_eq!(parse_constructor_pattern("Ok(Some(x))"), None);
    }

    #[test]
    fn pcp_malformed_no_close_paren_returns_none() {
        // `strip_suffix(')')` returns None → whole function returns None.
        assert_eq!(parse_constructor_pattern("Ok("), None);
        assert_eq!(parse_constructor_pattern("Some(x"), None);
    }

    // ── arm_payload_binding ────────────────────────────────────────────────

    #[test]
    fn apb_single_var() {
        assert_eq!(arm_payload_binding("Ok(x)"), Some("x"));
        assert_eq!(arm_payload_binding("Some(value)"), Some("value"));
        assert_eq!(arm_payload_binding("Err(e)"), Some("e"));
    }

    #[test]
    fn apb_tag_only_returns_none() {
        assert_eq!(arm_payload_binding("None"), None);
        assert_eq!(arm_payload_binding("True"), None);
    }

    #[test]
    fn apb_bare_wildcard_returns_none() {
        assert_eq!(arm_payload_binding("_"), None);
    }

    #[test]
    fn apb_inner_wildcard_returns_none() {
        // "Ok(_)" — the inner binding is "_", not a real variable.
        assert_eq!(arm_payload_binding("Ok(_)"), None);
    }

    #[test]
    fn apb_trims_inner_whitespace() {
        assert_eq!(arm_payload_binding("Ok( x )"), Some("x"));
        assert_eq!(arm_payload_binding("  Ok(x)  "), Some("x"));
    }

    #[test]
    fn apb_nested_constructor_returns_none() {
        assert_eq!(arm_payload_binding("Ok(Some(x))"), None);
    }

    #[test]
    fn apb_multi_binding_returns_none() {
        assert_eq!(arm_payload_binding("Pair(a, b)"), None);
    }

    #[test]
    fn apb_lowercase_start_returns_none() {
        assert_eq!(arm_payload_binding("ok(x)"), None);
        assert_eq!(arm_payload_binding(""), None);
    }

    #[test]
    fn apb_malformed_no_close_paren_returns_none() {
        // Fixed divergence: previously `arm_payload_binding("Ok(")` returned
        // Some("") because the old implementation used `unwrap_or(inner)`.
        // Now it delegates to `parse_constructor_pattern` which uses
        // `strip_suffix(')')?` and correctly returns None.
        assert_eq!(arm_payload_binding("Ok("), None);
    }

    // ── is_unsupported_pattern_shape ───────────────────────────────────────

    #[test]
    fn iups_record_syntax() {
        assert!(is_unsupported_pattern_shape("{name: x}"));
    }

    #[test]
    fn iups_nested_constructor() {
        assert!(is_unsupported_pattern_shape("Ok(Some(x))"));
    }

    #[test]
    fn iups_multi_binding() {
        assert!(is_unsupported_pattern_shape("Pair(a, b)"));
    }

    #[test]
    fn iups_single_binding_is_fine() {
        assert!(!is_unsupported_pattern_shape("Ok(x)"));
    }

    #[test]
    fn iups_tag_only_is_fine() {
        assert!(!is_unsupported_pattern_shape("None"));
    }

    #[test]
    fn iups_wildcard_is_fine() {
        assert!(!is_unsupported_pattern_shape("_"));
    }
}
