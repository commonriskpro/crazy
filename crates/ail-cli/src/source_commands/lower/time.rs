use super::*;

pub(super) fn lower_source_time_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((lowering, arity, usage)) = source_time_helper_lowering(&func) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TimeHelper,
            format!("{func} requires `{usage}`"),
        ));
    }
    if lowering == SourceTimeLowering::ClockNow {
        return Ok(Some("effect_call(clock.now, now)".to_string()));
    }
    let SourceTimeLowering::Pure(lowered_func) = lowering else {
        unreachable!("checked source time lowering")
    };
    Ok(Some(format!(
        "{lowered_func}({})",
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    )))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceTimeLowering {
    ClockNow,
    Pure(&'static str),
}

fn source_time_helper_lowering(func: &str) -> Option<(SourceTimeLowering, usize, &'static str)> {
    match func {
        "time.now" | "time_now" | "std.time.now" => {
            Some((SourceTimeLowering::ClockNow, 0, "time_now()"))
        }
        "time.duration_since" | "time_duration_since" | "std.time.duration_since" => Some((
            SourceTimeLowering::Pure("std.time.duration_since"),
            2,
            "time_duration_since(later_ms, earlier_ms)",
        )),
        "time.add_duration" | "time_add_duration" | "std.time.add_duration" => Some((
            SourceTimeLowering::Pure("std.time.add_duration"),
            2,
            "time_add_duration(instant_ms, duration_ms)",
        )),
        "time.instant_to_ms" | "time_instant_to_ms" | "std.time.instant_to_ms" => Some((
            SourceTimeLowering::Pure("std.time.instant_to_ms"),
            1,
            "time_instant_to_ms(instant_ms)",
        )),
        _ => None,
    }
}
