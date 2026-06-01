use super::*;

pub(super) fn lower_source_numeric_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((lowered_func, usage)) = source_numeric_helper_lowering(&func) else {
        return Ok(None);
    };
    if args.len() != 1 {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::NumericHelper,
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

fn source_numeric_helper_lowering(func: &str) -> Option<(&'static str, &'static str)> {
    match func {
        "numeric.narrow_to_i32" | "numeric_narrow_to_i32" | "std.numeric.narrow_to_i32" => {
            Some(("std.numeric.narrow_to_i32", "numeric_narrow_to_i32(value)"))
        }
        "numeric.narrow_to_u32" | "numeric_narrow_to_u32" | "std.numeric.narrow_to_u32" => {
            Some(("std.numeric.narrow_to_u32", "numeric_narrow_to_u32(value)"))
        }
        "numeric.narrow_to_u64" | "numeric_narrow_to_u64" | "std.numeric.narrow_to_u64" => {
            Some(("std.numeric.narrow_to_u64", "numeric_narrow_to_u64(value)"))
        }
        "numeric.narrow_to_i16" | "numeric_narrow_to_i16" | "std.numeric.narrow_to_i16" => {
            Some(("std.numeric.narrow_to_i16", "numeric_narrow_to_i16(value)"))
        }
        "numeric.narrow_to_u8" | "numeric_narrow_to_u8" | "std.numeric.narrow_to_u8" => {
            Some(("std.numeric.narrow_to_u8", "numeric_narrow_to_u8(value)"))
        }
        _ => None,
    }
}
