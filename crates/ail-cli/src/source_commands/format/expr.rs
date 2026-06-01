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
                "if {} {} else {}",
                format_source_expr(&args[0], module, constants),
                format_source_branch_block(&args[1], module, constants, ""),
                format_source_else_branch(&args[2], module, constants)
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

    if func == "unit" && args.is_empty() {
        return ("()".to_string(), CALL_PRECEDENCE);
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

    if matches!(func.as_str(), "text.len" | "text.length") && args.len() == 1 {
        return (
            format!(
                "text_length({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "log.write" && args.len() == 1 {
        return (
            format!(
                "log_write({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.is_empty" && args.len() == 1 {
        return (
            format!(
                "text_is_empty({})",
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

    if let Some((helper, arity)) = source_decimal_helper(&func)
        && args.len() == arity
    {
        return (
            format!(
                "{helper}({})",
                args.iter()
                    .map(|arg| format_source_expr(arg, module, constants))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            CALL_PRECEDENCE,
        );
    }

    if let Some((helper, arity)) = source_bytes_helper(&func)
        && args.len() == arity
    {
        return (
            format!(
                "{helper}({})",
                args.iter()
                    .map(|arg| format_source_expr(arg, module, constants))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            CALL_PRECEDENCE,
        );
    }

    if let Some((helper, arity)) = source_time_helper(&func)
        && args.len() == arity
    {
        return (
            format!(
                "{helper}({})",
                args.iter()
                    .map(|arg| format_source_expr(arg, module, constants))
                    .collect::<Vec<_>>()
                    .join(", ")
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

    if matches!(func.as_str(), "option.is_some" | "option.is_none") && args.len() == 1 {
        let helper = if func == "option.is_some" {
            "option_is_some"
        } else {
            "option_is_none"
        };
        return (
            format!(
                "{helper}({})",
                format_source_expr(&args[0], module, constants)
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

    if matches!(func.as_str(), "result.is_ok" | "result.is_err") && args.len() == 1 {
        let helper = if func == "result.is_ok" {
            "result_is_ok"
        } else {
            "result_is_err"
        };
        return (
            format!(
                "{helper}({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if matches!(func.as_str(), "list.push" | "list.concat") {
        let helper = match func.as_str() {
            "list.push" => "list_push",
            "list.concat" => "list_concat",
            _ => unreachable!("checked list helper"),
        };
        return (
            format!(
                "{helper}({})",
                args.iter()
                    .map(|arg| format_source_expr(arg, module, constants))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            CALL_PRECEDENCE,
        );
    }

    if matches!(
        func.as_str(),
        "queue.push_back"
            | "queue.pop_front"
            | "queue.peek_front"
            | "queue.length"
            | "queue.is_empty"
    ) {
        let helper = match func.as_str() {
            "queue.push_back" => "queue_push_back",
            "queue.pop_front" => "queue_pop_front",
            "queue.peek_front" => "queue_peek_front",
            "queue.length" => "queue_length",
            "queue.is_empty" => "queue_is_empty",
            _ => unreachable!("checked queue helper"),
        };
        return (
            format!(
                "{helper}({})",
                args.iter()
                    .map(|arg| format_source_expr(arg, module, constants))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            CALL_PRECEDENCE,
        );
    }

    if matches!(func.as_str(), "set.contains" | "set.length" | "set.insert") {
        let helper = match func.as_str() {
            "set.contains" => "set_contains",
            "set.length" => "set_length",
            "set.insert" => "set_insert",
            _ => unreachable!("checked set helper"),
        };
        return (
            format!(
                "{helper}({})",
                args.iter()
                    .map(|arg| format_source_expr(arg, module, constants))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            CALL_PRECEDENCE,
        );
    }

    if matches!(
        func.as_str(),
        "map.get" | "map.contains_key" | "map.length" | "map.insert"
    ) {
        let helper = match func.as_str() {
            "map.get" => "map_get",
            "map.contains_key" => "map_contains_key",
            "map.length" => "map_length",
            "map.insert" => "map_insert",
            _ => unreachable!("checked map helper"),
        };
        return (
            format!(
                "{helper}({})",
                args.iter()
                    .map(|arg| format_source_expr(arg, module, constants))
                    .collect::<Vec<_>>()
                    .join(", ")
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

    if func == "list.is_empty" && args.len() == 1 {
        return (
            format!(
                "list_is_empty({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "list.length" && args.len() == 1 {
        return (
            format!(
                "list_length({})",
                format_source_expr(&args[0], module, constants)
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

fn source_time_helper(func: &str) -> Option<(&'static str, usize)> {
    match func {
        "time.duration_since" | "std.time.duration_since" => Some(("time_duration_since", 2)),
        "time.add_duration" | "std.time.add_duration" => Some(("time_add_duration", 2)),
        "time.instant_to_ms" | "std.time.instant_to_ms" => Some(("time_instant_to_ms", 1)),
        _ => None,
    }
}

fn source_bytes_helper(func: &str) -> Option<(&'static str, usize)> {
    match func {
        "bytes.length" | "std.bytes.length" => Some(("bytes_length", 1)),
        "bytes.at" | "std.bytes.at" => Some(("bytes_at", 2)),
        "bytes.slice" | "std.bytes.slice" => Some(("bytes_slice", 3)),
        "bytes.concat" | "std.bytes.concat" => Some(("bytes_concat", 2)),
        "bytes.empty" | "std.bytes.empty" => Some(("bytes_empty", 1)),
        _ => None,
    }
}

fn source_decimal_helper(func: &str) -> Option<(&'static str, usize)> {
    match func {
        "decimal.from_int" | "std.decimal.from_int" => Some(("decimal_from_int", 1)),
        "decimal.rescale" | "std.decimal.rescale" => Some(("decimal_rescale", 2)),
        "decimal.add" | "std.decimal.add" => Some(("decimal_add", 2)),
        "decimal.sub" | "std.decimal.sub" => Some(("decimal_sub", 2)),
        "decimal.mul" | "std.decimal.mul" => Some(("decimal_mul", 2)),
        "decimal.is_negative" | "std.decimal.is_negative" => Some(("decimal_is_negative", 1)),
        "decimal.is_zero" | "std.decimal.is_zero" => Some(("decimal_is_zero", 1)),
        "decimal.non_negative" | "std.decimal.non_negative" => Some(("decimal_non_negative", 1)),
        _ => None,
    }
}

fn format_source_else_branch(
    expr: &str,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) -> String {
    let expr = expr.trim();
    if let Some((func, args)) = parse_source_call(expr)
        && func == "if"
        && args.len() == 3
        && !source_if_prefers_helper(&args)
    {
        return format!(
            "if {} {} else {}",
            format_source_expr(&args[0], module, constants),
            format_source_branch_block(&args[1], module, constants, ""),
            format_source_else_branch(&args[2], module, constants)
        );
    }
    format_source_branch_block(expr, module, constants, "")
}

fn format_source_branch_block(
    expr: &str,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
    closing_indent: &str,
) -> String {
    let (lets, final_expr) = source_let_chain(expr);
    if lets.is_empty() {
        return format!("{{ {} }}", format_source_expr(expr, module, constants));
    }

    let body_indent = format!("{closing_indent}  ");
    let mut out = String::from("{\n");
    for binding in lets {
        if binding.is_statement {
            out.push_str(&format!(
                "{body_indent}{}\n",
                format_source_expr(&binding.value, module, constants)
            ));
            continue;
        }
        let annotation = binding
            .ty
            .as_ref()
            .map(|ty| format!(": {ty}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "{body_indent}let {}{} = {}\n",
            binding.name,
            annotation,
            format_source_expr(&binding.value, module, constants)
        ));
    }
    out.push_str(&format!(
        "{body_indent}return {}\n{closing_indent}}}",
        format_source_expr(&final_expr, module, constants)
    ));
    out
}

fn source_if_prefers_helper(args: &[String]) -> bool {
    source_if_as_first_or(args).is_some()
        || source_if_as_last_or(args).is_some()
        || source_if_as_get_or(args).is_some()
        || source_if_as_list_get(args).is_some()
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
                "  {} => {}",
                format_source_match_pattern(&pair[0]),
                format_source_match_arm_block(&pair[1], module, constants)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "match {} {{\n{arms}\n}}",
        format_source_expr(&args[0], module, constants)
    )
}

fn format_source_match_arm_block(
    expr: &str,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) -> String {
    let (lets, _) = source_let_chain(expr);
    if lets.is_empty() {
        return format!(
            "{{\n    return {}\n  }}",
            format_source_expr(expr, module, constants)
        );
    }
    format_source_branch_block(expr, module, constants, "  ")
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
