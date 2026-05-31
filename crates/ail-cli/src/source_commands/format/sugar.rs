use super::*;

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

pub(super) fn source_if_as_list_get(args: &[String]) -> Option<(String, String)> {
    if args.len() != 3 {
        return None;
    }
    let (then_func, then_args) = parse_source_call(&args[1])?;
    if !matches!(then_func.as_str(), "some" | "Some") || then_args.len() != 1 {
        return None;
    }
    let (index_func, index_args) = parse_source_call(&then_args[0])?;
    if index_func != "index" || index_args.len() != 2 {
        return None;
    }
    if !source_expr_is_none(&args[2]) {
        return None;
    }
    let list = index_args[0].trim();
    let index = index_args[1].trim();
    let (cond_list, cond_index) = source_get_or_guard_parts(&args[0])?;
    if cond_list.trim() != list || cond_index.trim() != index {
        return None;
    }
    Some((list.to_string(), index.to_string()))
}

fn source_expr_is_none(expr: &str) -> bool {
    let expr = expr.trim();
    if matches!(expr, "none" | "None" | "none()" | "None()") {
        return true;
    }
    let Some((func, args)) = parse_source_call(expr) else {
        return false;
    };
    matches!(func.as_str(), "none" | "None") && args.is_empty()
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

pub(super) fn source_match_as_result_unwrap_or(args: &[String]) -> Option<(String, String)> {
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
            Some(("Ok", Some(binding))) if body == binding => success = Some(()),
            Some(("Err", Some("_"))) => fallback = Some(body.to_string()),
            _ => return None,
        }
    }
    if success.is_some() {
        Some((scrutinee, fallback?))
    } else {
        None
    }
}

pub(super) fn source_match_as_option_ok_or(args: &[String]) -> Option<(String, String)> {
    if args.len() != 5 {
        return None;
    }
    let scrutinee = args[0].trim().to_string();
    let arms = args[1..]
        .chunks_exact(2)
        .map(|pair| (pair[0].trim(), pair[1].trim()))
        .collect::<Vec<_>>();

    let mut success = None;
    let mut error = None;
    for (pattern, body) in arms {
        match source_constructor_pattern(pattern) {
            Some(("Some", Some(binding))) if source_body_is_ok_binding(body, binding) => {
                success = Some(())
            }
            Some(("None", None)) => error = source_body_err_arg(body),
            _ => return None,
        }
    }
    if success.is_some() {
        Some((scrutinee, error?))
    } else {
        None
    }
}

fn source_body_is_ok_binding(body: &str, binding: &str) -> bool {
    let Some((func, args)) = parse_source_call(body) else {
        return false;
    };
    matches!(func.as_str(), "ok" | "Ok") && args.len() == 1 && args[0].trim() == binding
}

fn source_body_err_arg(body: &str) -> Option<String> {
    let (func, args) = parse_source_call(body)?;
    if matches!(func.as_str(), "err" | "Err") && args.len() == 1 {
        Some(args[0].trim().to_string())
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
