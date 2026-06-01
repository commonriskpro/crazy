use super::*;

pub(super) fn lower_source_bytes_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((lowered_func, arity, usage)) = source_bytes_helper_lowering(&func) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::BytesHelper,
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

fn source_bytes_helper_lowering(func: &str) -> Option<(&'static str, usize, &'static str)> {
    match func {
        "bytes.length" | "bytes_length" | "std.bytes.length" => {
            Some(("std.bytes.length", 1, "bytes_length(bytes)"))
        }
        "bytes.at" | "bytes_at" | "std.bytes.at" => {
            Some(("std.bytes.at", 2, "bytes_at(bytes, index)"))
        }
        "bytes.slice" | "bytes_slice" | "std.bytes.slice" => {
            Some(("std.bytes.slice", 3, "bytes_slice(bytes, start, end)"))
        }
        "bytes.concat" | "bytes_concat" | "std.bytes.concat" => {
            Some(("std.bytes.concat", 2, "bytes_concat(left, right)"))
        }
        "bytes.empty" | "bytes_empty" | "std.bytes.empty" => {
            Some(("std.bytes.empty", 1, "bytes_empty(bytes)"))
        }
        _ => None,
    }
}
