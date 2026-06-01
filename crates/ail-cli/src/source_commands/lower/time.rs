use super::*;

pub(super) fn lower_source_time_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((lowered_func, arity, usage)) = source_time_helper_lowering(&func) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TimeHelper,
            format!("{func} requires `{usage}`"),
        ));
    }
    Ok(Some(format!(
        "{lowered_func}({})",
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    )))
}

fn source_time_helper_lowering(func: &str) -> Option<(&'static str, usize, &'static str)> {
    match func {
        "time.duration_since" | "time_duration_since" | "std.time.duration_since" => Some((
            "std.time.duration_since",
            2,
            "time_duration_since(later_ms, earlier_ms)",
        )),
        "time.add_duration" | "time_add_duration" | "std.time.add_duration" => Some((
            "std.time.add_duration",
            2,
            "time_add_duration(instant_ms, duration_ms)",
        )),
        "time.instant_to_ms" | "time_instant_to_ms" | "std.time.instant_to_ms" => Some((
            "std.time.instant_to_ms",
            1,
            "time_instant_to_ms(instant_ms)",
        )),
        _ => None,
    }
}
