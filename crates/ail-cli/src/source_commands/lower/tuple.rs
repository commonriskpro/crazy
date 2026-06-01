use super::*;

pub(super) fn lower_source_tuple_accessor_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((canonical, expected, usage)) = tuple_accessor_lowering(&func) else {
        return Ok(None);
    };
    if args.len() != expected {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TupleHelper,
            format!("{func} requires `{usage}`"),
        ));
    }
    Ok(Some(format!(
        "{canonical}({})",
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    )))
}

fn tuple_accessor_lowering(func: &str) -> Option<(&'static str, usize, &'static str)> {
    match func {
        "tuple.length" | "tuple_length" => Some(("tuple.length", 1, "tuple_length(tuple)")),
        "tuple.get" | "tuple_get" => Some(("tuple.get", 2, "tuple_get(tuple, index)")),
        "tuple.first" | "tuple_first" => Some(("tuple.first", 1, "tuple_first(tuple)")),
        "tuple.second" | "tuple_second" => Some(("tuple.second", 1, "tuple_second(tuple)")),
        _ => None,
    }
}
