use super::model::*;
use super::parse::*;
use super::syntax::*;
use super::validate::*;
use super::*;

pub(super) fn render_source_import(out: &mut String, import: &str) {
    out.push_str(&format!("use \"{import}\"\n"));
}

pub(super) fn render_source_module(out: &mut String, module: &str) {
    out.push_str(&format!("module {module}\n"));
}

pub(super) fn render_source_capability(out: &mut String, capability: &str) {
    out.push_str(&format!("capability {capability}\n"));
}

pub(super) fn render_source_const(
    out: &mut String,
    constant: &SourceConst,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) {
    let name = render_source_decl_name(
        constant.name.strip_prefix("fn.").unwrap_or(&constant.name),
        module,
    );
    out.push_str(&format!(
        "const {name}: {} = {}\n",
        constant.return_type,
        format_source_expr(&constant.body, module, constants)
    ));
}

pub(super) fn render_source_function(
    out: &mut String,
    function: &SourceFunction,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) {
    let name = render_source_decl_name(
        function.name.strip_prefix("fn.").unwrap_or(&function.name),
        module,
    );
    let params = function
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param.ty))
        .collect::<Vec<_>>()
        .join(", ");
    let signature = format!("fn {name}({params}) -> {}", function.return_type);

    let (lets, final_expr) = source_let_chain(&function.body);
    if lets.is_empty() {
        out.push_str(&format!(
            "{signature} = {}\n",
            format_source_expr(&function.body, module, constants)
        ));
        return;
    }

    out.push_str(&format!("{signature} {{\n"));
    for binding in lets {
        let annotation = binding
            .ty
            .as_ref()
            .map(|ty| format!(": {ty}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "  let {}{} = {}\n",
            binding.name,
            annotation,
            format_source_expr(&binding.value, module, constants)
        ));
    }
    out.push_str(&format!(
        "  return {}\n",
        format_source_expr(&final_expr, module, constants)
    ));
    out.push_str("}\n");
}

pub(super) fn render_source_test(
    out: &mut String,
    test: &SourceTest,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) {
    let name = render_source_decl_name(
        test.name.strip_prefix("test.").unwrap_or(&test.name),
        module,
    );
    if test.return_type == "Bool" {
        out.push_str(&format!(
            "test {name} = {}\n",
            format_source_expr(&test.body, module, constants)
        ));
    } else {
        out.push_str(&format!(
            "test {name} -> {} = {}\n",
            test.return_type,
            format_source_expr(&test.body, module, constants)
        ));
    }
}

pub(super) fn render_source_grant(out: &mut String, grant: &SourceGrant, module: Option<&str>) {
    let raw_target = grant
        .target
        .strip_prefix("fn.")
        .or_else(|| grant.target.strip_prefix("test."))
        .unwrap_or(&grant.target);
    let target = render_source_decl_name(raw_target, module);
    out.push_str(&format!("grant {target} {}\n", grant.capability));
}

pub(super) fn render_source_decl_name(name: &str, module: Option<&str>) -> String {
    module
        .and_then(|module| name.strip_prefix(&format!("{module}.")))
        .unwrap_or(name)
        .to_string()
}

pub(super) fn format_source_call_name(func: &str, module: Option<&str>) -> String {
    render_source_decl_name(func, module)
}

pub(super) struct SourceLetBinding {
    name: String,
    ty: Option<String>,
    value: String,
}

pub(super) fn source_let_chain(body: &str) -> (Vec<SourceLetBinding>, String) {
    let mut lets = Vec::new();
    let mut current = body.trim().to_string();
    while let Some((func, args)) = parse_source_call(&current) {
        match (func.as_str(), args.as_slice()) {
            ("let", [name, value, next]) if is_source_local_ident(name) => {
                lets.push(SourceLetBinding {
                    name: name.clone(),
                    ty: None,
                    value: value.clone(),
                });
                current = next.clone();
            }
            ("let_typed", [name, ty, _line, value, next]) if is_source_local_ident(name) => {
                lets.push(SourceLetBinding {
                    name: name.clone(),
                    ty: Some(ty.clone()),
                    value: value.clone(),
                });
                current = next.clone();
            }
            _ => break,
        }
    }
    (lets, current)
}

pub(super) fn format_source_expr(
    expr: &str,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) -> String {
    format_source_expr_node(expr, module, constants).0
}

pub(super) fn format_source_expr_node(
    expr: &str,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) -> (String, u8) {
    const IF_PRECEDENCE: u8 = 0;
    const UNARY_PRECEDENCE: u8 = 7;
    const CALL_PRECEDENCE: u8 = 8;

    let expr = expr.trim();
    let Some((func, args)) = parse_source_call(expr) else {
        return (expr.to_string(), CALL_PRECEDENCE);
    };

    if args.is_empty()
        && let Some(target) = source_const_reference_target(&func, constants)
    {
        return (render_source_decl_name(&target, module), CALL_PRECEDENCE);
    }

    if func == "if" && args.len() == 3 {
        if let Some((list, fallback)) = source_if_as_first_or(&args) {
            return (
                format!(
                    "first_or({}, {})",
                    format_source_expr(&list, module, constants),
                    format_source_expr(&fallback, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        if let Some((list, fallback)) = source_if_as_last_or(&args) {
            return (
                format!(
                    "last_or({}, {})",
                    format_source_expr(&list, module, constants),
                    format_source_expr(&fallback, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        if let Some((list, index, fallback)) = source_if_as_get_or(&args) {
            return (
                format!(
                    "get_or({}, {}, {})",
                    format_source_expr(&list, module, constants),
                    format_source_expr(&index, module, constants),
                    format_source_expr(&fallback, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        return (
            format!(
                "if {} {{ {} }} else {{ {} }}",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            IF_PRECEDENCE,
        );
    }

    if func == "match" && args.len() >= 3 && args.len() % 2 == 1 {
        if let Some((helper, value)) = source_match_as_option_predicate(&args) {
            return (
                format!(
                    "{helper}({})",
                    format_source_expr(&value, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        if let Some((helper, value)) = source_match_as_result_predicate(&args) {
            return (
                format!(
                    "{helper}({})",
                    format_source_expr(&value, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        if let Some((value, fallback)) = source_match_as_unwrap_or(&args) {
            return (
                format!(
                    "unwrap_or({}, {})",
                    format_source_expr(&value, module, constants),
                    format_source_expr(&fallback, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        return (
            format_source_match_expr(&args, module, constants),
            IF_PRECEDENCE,
        );
    }

    if func == "none" && args.is_empty() {
        return ("None".to_string(), CALL_PRECEDENCE);
    }

    if matches!(func.as_str(), "some" | "ok" | "err") && args.len() == 1 {
        let constructor = match func.as_str() {
            "some" => "Some",
            "ok" => "Ok",
            "err" => "Err",
            _ => unreachable!("checked source constructor"),
        };
        return (
            format!(
                "{constructor}({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "not" && args.len() == 1 {
        return (
            format!(
                "!{}",
                format_source_child_expr(&args[0], module, constants, UNARY_PRECEDENCE, false)
            ),
            UNARY_PRECEDENCE,
        );
    }

    if func == "sub" && args.len() == 2 && args[0].trim() == "0" {
        return (
            format!(
                "-{}",
                format_source_child_expr(&args[1], module, constants, UNARY_PRECEDENCE, false)
            ),
            UNARY_PRECEDENCE,
        );
    }

    if func == "eq"
        && args.len() == 2
        && let Some(value) = source_eq_as_is_empty(&args)
    {
        return (
            format!(
                "is_empty({})",
                format_source_expr(&value, module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if matches!(func.as_str(), "int.min" | "int.max") && args.len() == 2 {
        let helper = if func == "int.min" {
            "int_min"
        } else {
            "int_max"
        };
        return (
            format!(
                "{helper}({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.abs_or" && args.len() == 2 {
        return (
            format!(
                "int_abs_or({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.neg_or" && args.len() == 2 {
        return (
            format!(
                "int_neg_or({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.saturating_add" && args.len() == 2 {
        return (
            format!(
                "int_saturating_add({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.saturating_sub" && args.len() == 2 {
        return (
            format!(
                "int_saturating_sub({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.saturating_mul" && args.len() == 2 {
        return (
            format!(
                "int_saturating_mul({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.wrapping_add" && args.len() == 2 {
        return (
            format!(
                "int_wrapping_add({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.wrapping_sub" && args.len() == 2 {
        return (
            format!(
                "int_wrapping_sub({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.wrapping_mul" && args.len() == 2 {
        return (
            format!(
                "int_wrapping_mul({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.bit_and" && args.len() == 2 {
        return (
            format!(
                "int_bit_and({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.bit_or" && args.len() == 2 {
        return (
            format!(
                "int_bit_or({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.bit_xor" && args.len() == 2 {
        return (
            format!(
                "int_bit_xor({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.shift_left" && args.len() == 2 {
        return (
            format!(
                "int_shift_left({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.shift_right" && args.len() == 2 {
        return (
            format!(
                "int_shift_right({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.shift_right_unsigned" && args.len() == 2 {
        return (
            format!(
                "int_shift_right_unsigned({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.bit_not" && args.len() == 1 {
        return (
            format!(
                "int_bit_not({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.wrapping_neg" && args.len() == 1 {
        return (
            format!(
                "int_wrapping_neg({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.saturating_neg" && args.len() == 1 {
        return (
            format!(
                "int_saturating_neg({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.clamp" && args.len() == 3 {
        return (
            format!(
                "int_clamp({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.add_or" && args.len() == 3 {
        return (
            format!(
                "int_add_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.sub_or" && args.len() == 3 {
        return (
            format!(
                "int_sub_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.mul_or" && args.len() == 3 {
        return (
            format!(
                "int_mul_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.div_or" && args.len() == 3 {
        return (
            format!(
                "int_div_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.rem_or" && args.len() == 3 {
        return (
            format!(
                "int_rem_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.eq" && args.len() == 2 {
        return (
            format!(
                "text_eq({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.trim" && args.len() == 1 {
        return (
            format!(
                "text_trim({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.contains" && args.len() == 2 {
        return (
            format!(
                "text_contains({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.index_of" && args.len() == 2 {
        return (
            format!(
                "text_index_of({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.parse_int_or" && args.len() == 2 {
        return (
            format!(
                "text_parse_int_or({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.byte_at_or" && args.len() == 3 {
        return (
            format!(
                "text_byte_at_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.slice" && args.len() == 3 {
        return (
            format!(
                "text_slice({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.replace_first" && args.len() == 3 {
        return (
            format!(
                "text_replace_first({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.starts_with" && args.len() == 2 {
        return (
            format!(
                "text_starts_with({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.ends_with" && args.len() == 2 {
        return (
            format!(
                "text_ends_with({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "list" {
        return (
            format!(
                "[{}]",
                args.iter()
                    .map(|arg| format_source_expr(arg, module, constants))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "record"
        && args.len().is_multiple_of(2)
        && args
            .chunks_exact(2)
            .all(|pair| is_source_local_ident(&pair[0]))
    {
        let fields = args
            .chunks_exact(2)
            .map(|pair| {
                format!(
                    "{}: {}",
                    pair[0].trim(),
                    format_source_expr(&pair[1], module, constants)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return (
            if fields.is_empty() {
                "{}".to_string()
            } else {
                format!("{{ {fields} }}")
            },
            CALL_PRECEDENCE,
        );
    }

    if func == "index" && args.len() == 2 {
        return (
            format!(
                "{}[{}]",
                format_source_child_expr(&args[0], module, constants, CALL_PRECEDENCE, false),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "field" && args.len() == 2 {
        return (
            format!(
                "{}.{}",
                format_source_child_expr(&args[0], module, constants, CALL_PRECEDENCE, false),
                args[1].trim()
            ),
            CALL_PRECEDENCE,
        );
    }

    if let Some((base, fields)) = collect_source_update_literal(expr) {
        let mut parts = vec![format!(
            "...{}",
            format_source_expr(&base, module, constants)
        )];
        parts.extend(fields.iter().map(|(field, value)| {
            format!("{field}: {}", format_source_expr(value, module, constants))
        }));
        return (format!("{{ {} }}", parts.join(", ")), CALL_PRECEDENCE);
    }

    if args.len() == 2 {
        if let Some((operator, precedence)) = source_infix_operator(&func) {
            return (
                format!(
                    "{} {operator} {}",
                    format_source_child_expr(&args[0], module, constants, precedence, false),
                    format_source_child_expr(&args[1], module, constants, precedence, true)
                ),
                precedence,
            );
        }
    }

    (
        format!(
            "{}({})",
            format_source_call_name(&func, module),
            args.iter()
                .map(|arg| format_source_expr(arg, module, constants))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        CALL_PRECEDENCE,
    )
}

pub(super) fn source_if_as_first_or(args: &[String]) -> Option<(String, String)> {
    if args.len() != 3 {
        return None;
    }
    let (cond_func, cond_args) = parse_source_call(&args[0])?;
    if cond_func != "gt" || cond_args.len() != 2 || cond_args[1].trim() != "0" {
        return None;
    }
    let (len_func, len_args) = parse_source_call(&cond_args[0])?;
    if len_func != "len" || len_args.len() != 1 {
        return None;
    }
    let list = len_args[0].trim();
    let (index_func, index_args) = parse_source_call(&args[1])?;
    if index_func != "index"
        || index_args.len() != 2
        || index_args[0].trim() != list
        || index_args[1].trim() != "0"
    {
        return None;
    }
    Some((list.to_string(), args[2].trim().to_string()))
}

pub(super) fn source_if_as_last_or(args: &[String]) -> Option<(String, String)> {
    if args.len() != 3 {
        return None;
    }
    let (cond_func, cond_args) = parse_source_call(&args[0])?;
    if cond_func != "gt" || cond_args.len() != 2 || cond_args[1].trim() != "0" {
        return None;
    }
    let (len_func, len_args) = parse_source_call(&cond_args[0])?;
    if len_func != "len" || len_args.len() != 1 {
        return None;
    }
    let list = len_args[0].trim();
    let (index_func, index_args) = parse_source_call(&args[1])?;
    if index_func != "index" || index_args.len() != 2 || index_args[0].trim() != list {
        return None;
    }
    let (sub_func, sub_args) = parse_source_call(&index_args[1])?;
    if sub_func != "sub" || sub_args.len() != 2 || sub_args[1].trim() != "1" {
        return None;
    }
    let (idx_len_func, idx_len_args) = parse_source_call(&sub_args[0])?;
    if idx_len_func != "len" || idx_len_args.len() != 1 || idx_len_args[0].trim() != list {
        return None;
    }
    Some((list.to_string(), args[2].trim().to_string()))
}

pub(super) fn source_if_as_get_or(args: &[String]) -> Option<(String, String, String)> {
    if args.len() != 3 {
        return None;
    }
    let (then_func, then_args) = parse_source_call(&args[1])?;
    if then_func != "index" || then_args.len() != 2 {
        return None;
    }
    let list = then_args[0].trim();
    let index = then_args[1].trim();
    let (cond_list, cond_index) = source_get_or_guard_parts(&args[0])?;
    if cond_list.trim() != list || cond_index.trim() != index {
        return None;
    }
    Some((
        list.to_string(),
        index.to_string(),
        args[2].trim().to_string(),
    ))
}

pub(super) fn source_get_or_guard_parts(cond: &str) -> Option<(String, String)> {
    let (func, args) = parse_source_call(cond)?;
    if func != "and" || args.len() != 2 {
        return None;
    }
    let ge = source_get_or_ge_zero(&args[0]).or_else(|| source_get_or_ge_zero(&args[1]))?;
    let (lt_index, lt_list) =
        source_get_or_lt_len(&args[0]).or_else(|| source_get_or_lt_len(&args[1]))?;
    if ge.trim() == lt_index.trim() {
        Some((lt_list, ge))
    } else {
        None
    }
}

pub(super) fn source_get_or_ge_zero(expr: &str) -> Option<String> {
    let (func, args) = parse_source_call(expr)?;
    if func == "ge" && args.len() == 2 && args[1].trim() == "0" {
        Some(args[0].trim().to_string())
    } else {
        None
    }
}

pub(super) fn source_get_or_lt_len(expr: &str) -> Option<(String, String)> {
    let (func, args) = parse_source_call(expr)?;
    if func != "lt" || args.len() != 2 {
        return None;
    }
    let (len_func, len_args) = parse_source_call(&args[1])?;
    if len_func == "len" && len_args.len() == 1 {
        Some((args[0].trim().to_string(), len_args[0].trim().to_string()))
    } else {
        None
    }
}

pub(super) fn source_eq_as_is_empty(args: &[String]) -> Option<String> {
    if args.len() != 2 {
        return None;
    }
    source_len_zero_arg(&args[0], &args[1]).or_else(|| source_len_zero_arg(&args[1], &args[0]))
}

pub(super) fn source_len_zero_arg(len_expr: &str, zero_expr: &str) -> Option<String> {
    if zero_expr.trim() != "0" {
        return None;
    }
    let (func, args) = parse_source_call(len_expr.trim())?;
    if func == "len" && args.len() == 1 {
        Some(args[0].trim().to_string())
    } else {
        None
    }
}

pub(super) fn source_match_as_option_predicate(args: &[String]) -> Option<(&'static str, String)> {
    if args.len() != 5 {
        return None;
    }
    let scrutinee = args[0].trim().to_string();
    let arms = args[1..]
        .chunks_exact(2)
        .map(|pair| (pair[0].trim(), pair[1].trim()))
        .collect::<Vec<_>>();

    let mut some_body = None;
    let mut none_body = None;
    for (pattern, body) in arms {
        match source_constructor_pattern(pattern) {
            Some(("Some", Some("_"))) => some_body = Some(body),
            Some(("None", None)) => none_body = Some(body),
            _ => return None,
        }
    }

    match (some_body?, none_body?) {
        ("true", "false") => Some(("is_some", scrutinee)),
        ("false", "true") => Some(("is_none", scrutinee)),
        _ => None,
    }
}

pub(super) fn source_match_as_result_predicate(args: &[String]) -> Option<(&'static str, String)> {
    if args.len() != 5 {
        return None;
    }
    let scrutinee = args[0].trim().to_string();
    let arms = args[1..]
        .chunks_exact(2)
        .map(|pair| (pair[0].trim(), pair[1].trim()))
        .collect::<Vec<_>>();

    let mut ok_body = None;
    let mut err_body = None;
    for (pattern, body) in arms {
        match source_constructor_pattern(pattern) {
            Some(("Ok", Some("_"))) => ok_body = Some(body),
            Some(("Err", Some("_"))) => err_body = Some(body),
            _ => return None,
        }
    }

    match (ok_body?, err_body?) {
        ("true", "false") => Some(("is_ok", scrutinee)),
        ("false", "true") => Some(("is_err", scrutinee)),
        _ => None,
    }
}

pub(super) fn source_match_as_unwrap_or(args: &[String]) -> Option<(String, String)> {
    if args.len() != 5 {
        return None;
    }
    let scrutinee = args[0].trim().to_string();
    let arms = args[1..]
        .chunks_exact(2)
        .map(|pair| (pair[0].trim(), pair[1].trim()))
        .collect::<Vec<_>>();

    let mut success = None;
    let mut fallback = None;
    for (pattern, body) in arms {
        match source_constructor_pattern(pattern) {
            Some(("Some", Some(binding))) if body == binding => success = Some(()),
            Some(("None", None)) => fallback = Some(body.to_string()),
            _ => return None,
        }
    }
    if success.is_some() {
        Some((scrutinee, fallback?))
    } else {
        None
    }
}

pub(super) fn collect_source_update_literal(expr: &str) -> Option<(String, Vec<(String, String)>)> {
    let (func, args) = parse_source_call(expr)?;
    if func != "update" || args.len() != 3 || !is_source_local_ident(&args[1]) {
        return None;
    }
    let field = args[1].trim().to_string();
    let value = args[2].trim().to_string();
    if let Some((base, mut fields)) = collect_source_update_literal(&args[0]) {
        fields.push((field, value));
        Some((base, fields))
    } else {
        Some((args[0].trim().to_string(), vec![(field, value)]))
    }
}

pub(super) fn source_infix_operator(func: &str) -> Option<(&'static str, u8)> {
    match func {
        "or" => Some(("||", 1)),
        "and" => Some(("&&", 2)),
        "eq" => Some(("==", 3)),
        "ne" => Some(("!=", 3)),
        "gt" => Some((">", 4)),
        "ge" => Some((">=", 4)),
        "lt" => Some(("<", 4)),
        "le" => Some(("<=", 4)),
        "concat" => Some(("++", 5)),
        "add" => Some(("+", 5)),
        "sub" => Some(("-", 5)),
        "mul" => Some(("*", 6)),
        "div" => Some(("/", 6)),
        "mod" => Some(("%", 6)),
        _ => None,
    }
}

pub(super) fn format_source_match_expr(
    args: &[String],
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) -> String {
    let arms = args[1..]
        .chunks_exact(2)
        .map(|pair| {
            format!(
                "{} => {}",
                pair[0].trim(),
                format_source_expr(&pair[1], module, constants)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "match {} {{ {arms} }}",
        format_source_expr(&args[0], module, constants)
    )
}

pub(super) fn format_source_child_expr(
    expr: &str,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
    parent_precedence: u8,
    parenthesize_equal_precedence: bool,
) -> String {
    let (formatted, precedence) = format_source_expr_node(expr, module, constants);
    if precedence < parent_precedence
        || (parenthesize_equal_precedence && precedence == parent_precedence)
    {
        format!("({formatted})")
    } else {
        formatted
    }
}
