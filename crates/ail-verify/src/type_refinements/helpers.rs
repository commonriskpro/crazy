use crate::type_checker::TypeContext;

// ── Helpers ──────────────────────────────────────────────────────────────

/// Attempt to infer a return type from a `body_expr` string using local
/// pattern matching.  Returns `None` when the expression form is unrecognized.
pub(super) fn infer_expr_type(body: &str, ctx: &TypeContext<'_>) -> Option<String> {
    if body.parse::<i64>().is_ok() {
        return Some("Int".to_string());
    }

    if body == "true" || body == "false" {
        return Some("Bool".to_string());
    }

    if (body.starts_with("if ") || body.starts_with("if("))
        && let Some(inferred) = infer_if_expr_type(body, ctx)
    {
        return Some(inferred);
    }

    let callee_name = body
        .split(|c: char| c == '(' || c.is_whitespace())
        .next()
        .unwrap_or(body)
        .trim();

    if !callee_name.is_empty()
        && let Some(node) = ctx.get_by_name(callee_name)
        && let Some(rt) = &node.return_type
    {
        return Some(rt.clone());
    }

    None
}

/// Infer the type of an if-expression by examining its else branch.
fn infer_if_expr_type(body: &str, ctx: &TypeContext<'_>) -> Option<String> {
    let else_pos = body.find("else")?;
    let after_else = body[else_pos + 4..].trim();

    let else_body = if after_else.starts_with('{') {
        after_else
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .map(str::trim)
            .unwrap_or(after_else)
    } else {
        after_else
    };

    infer_expr_type(else_body, ctx)
}

/// Returns `true` when `name` is a simple decidable identifier:
/// letters, digits, or underscores only.
pub(crate) fn is_simple_identifier(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Returns `true` when `ty` is a valid decidable ConstParam value:
/// either an all-digit numeric literal or a simple identifier.
pub(super) fn is_const_param_value(ty: &str) -> bool {
    if ty.is_empty() {
        return false;
    }
    if ty.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    is_simple_identifier(ty)
}

/// Split an associated type reference `"Interface::AssocName"` into
/// `("Interface", "AssocName")`.
///
/// Returns `(input, "")` if the input does not contain `"::"`.
pub(super) fn split_assoc_type(ty: &str) -> (&str, &str) {
    if let Some(pos) = ty.find("::") {
        (&ty[..pos], &ty[pos + 2..])
    } else {
        (ty, "")
    }
}

/// Attempt to resolve an associated type for `arg_ty` by scanning the
/// `interface_impls` of the corresponding node in `ctx`.
pub(super) fn resolve_assoc_type<'a>(
    ctx: &TypeContext<'a>,
    arg_ty: &str,
    interface_base: &str,
    assoc_name: &str,
) -> Option<String> {
    let node = ctx.get_by_name(arg_ty)?;
    let impls = node.interface_impls.as_ref()?;
    impls.iter().find_map(|impl_meta| {
        if impl_meta.interface.starts_with(interface_base) {
            impl_meta
                .associated_types
                .iter()
                .find(|at| at.name == assoc_name)
                .map(|at| at.ty.clone())
        } else {
            None
        }
    })
}
