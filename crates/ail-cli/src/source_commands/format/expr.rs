use super::*;

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
        if let Some((list, index)) = source_if_as_list_get(&args) {
            return (
                format!(
                    "list_get({}, {})",
                    format_source_expr(&list, module, constants),
                    format_source_expr(&index, module, constants)
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
        if let Some((value, fallback)) = source_match_as_result_unwrap_or(&args) {
            return (
                format!(
                    "result_unwrap_or({}, {})",
                    format_source_expr(&value, module, constants),
                    format_source_expr(&fallback, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        if let Some((value, error)) = source_match_as_option_ok_or(&args) {
            return (
                format!(
                    "ok_or({}, {})",
                    format_source_expr(&value, module, constants),
                    format_source_expr(&error, module, constants)
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

    if func == "option.unwrap_or" && args.len() == 2 {
        return (
            format!(
                "option_unwrap_or({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "option.ok_or" && args.len() == 2 {
        return (
            format!(
                "ok_or({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "result.unwrap_or" && args.len() == 2 {
        return (
            format!(
                "result_unwrap_or({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "list.get" && args.len() == 2 {
        return (
            format!(
                "list_get({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "tuple.length" && args.len() == 1 {
        return (
            format!(
                "tuple_length({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "tuple.get" && args.len() == 2 {
        return (
            format!(
                "tuple_get({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "tuple.first" && args.len() == 1 {
        return (
            format!(
                "tuple_first({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "tuple.second" && args.len() == 1 {
        return (
            format!(
                "tuple_second({})",
                format_source_expr(&args[0], module, constants)
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
        let mut fields = args
            .chunks_exact(2)
            .map(|pair| {
                (
                    pair[0].trim().to_string(),
                    format_source_expr(&pair[1], module, constants),
                )
            })
            .collect::<Vec<_>>();
        fields.sort_by(|left, right| left.0.cmp(&right.0));
        let fields = fields
            .into_iter()
            .map(|(field, value)| format!("{field}: {value}"))
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
                format_source_match_pattern(&pair[0]),
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

fn format_source_match_pattern(pattern: &str) -> String {
    let pattern = pattern.trim();
    if let Some((tag, binding)) = source_constructor_pattern(pattern) {
        return match (tag, binding) {
            ("Some" | "Ok" | "Err", Some(binding)) => format!("{tag}({})", binding.trim()),
            ("None", None) => "None".to_string(),
            _ => pattern.to_string(),
        };
    }

    if matches!(pattern, "none" | "none()") {
        return "None".to_string();
    }

    for (lower, canonical) in [("some", "Some"), ("ok", "Ok"), ("err", "Err")] {
        let prefix = format!("{lower}(");
        if let Some(inner) = pattern
            .strip_prefix(prefix.as_str())
            .and_then(|rest| rest.strip_suffix(')'))
        {
            return format!("{canonical}({})", inner.trim());
        }
    }

    pattern.to_string()
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
