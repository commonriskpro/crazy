use super::*;

// ── normalize_id ──────────────────────────────────────────────────────────

/// Normalize an ACL identifier to canonical lower_snake form.
///
/// Rules (from spec):
/// - `Fn.CartTotal`    → `fn.cart_total`   (upper namespace + PascalCase → lower.snake)
/// - `fn.cart-total`   → `fn.cart_total`   (kebab → snake)
/// - `fn.cart_total`   → `fn.cart_total`   (already canonical)
/// - `type.CartItem`   → `type.CartItem`   (type. namespace: PascalCase preserved)
/// - `handler.StripePayment` → `handler.StripePayment` (handler. namespace preserved)
///
/// Namespaces that use PascalCase by convention (`type.`, `handler.`,
/// `boundary.`) are left with their original casing in the local part.
/// All other namespaces are lowercased with underscores.
pub fn normalize_id(id: &str) -> String {
    let dot = match id.find('.') {
        Some(p) => p,
        None => return id.to_string(), // no namespace — return as-is
    };
    let ns = &id[..dot];
    let local = &id[dot + 1..];

    // Namespaces whose local part keeps PascalCase by spec convention.
    const PASCAL_NAMESPACES: &[&str] = &["type", "handler", "boundary"];

    let ns_lower = ns.to_lowercase();
    let local_normalized = if PASCAL_NAMESPACES.contains(&ns_lower.as_str()) {
        // Preserve PascalCase in the local part; still normalize kebab → snake.
        local.replace('-', "_")
    } else {
        // Lower namespace + snake_case local part.
        pascal_to_snake(local).replace('-', "_")
    };

    format!("{}.{}", ns_lower, local_normalized)
}

/// Convert a PascalCase string to lower_snake_case.
///
/// `CartTotal` → `cart_total`
/// `cart_total` → `cart_total` (already snake)
fn pascal_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.char_indices() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

// ── normalize_op_args ─────────────────────────────────────────────────────

/// Normalize ID-valued op arguments in place.
///
/// Keys that conventionally carry node identifiers (`id`, `target`, `source`,
/// `to`, `from`) are normalized via `normalize_id`. Other values (type
/// expressions, free strings) are left as-is.
const ID_ARG_KEYS: &[&str] = &["id", "target", "source", "to", "from"];

pub(super) fn normalize_op_args(args: &mut OpArgs) {
    for key in ID_ARG_KEYS {
        if let Some(v) = args.get_mut(*key) {
            let normalized = normalize_id(v);
            *v = normalized;
        }
    }
}
