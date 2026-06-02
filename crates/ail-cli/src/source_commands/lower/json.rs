use super::*;

pub(super) fn lower_source_json_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((lowered_func, usage)) = source_json_helper_lowering(&func) else {
        return Ok(None);
    };
    if args.len() != 1 {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::JsonHelper,
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

fn source_json_helper_lowering(func: &str) -> Option<(&'static str, &'static str)> {
    match func {
        "json.parse" | "json_parse" | "std.json.parse" => {
            Some(("std.json.parse", "json_parse(text)"))
        }
        "json.stringify" | "json_stringify" | "std.json.stringify" => {
            Some(("std.json.stringify", "json_stringify(value)"))
        }
        _ => None,
    }
}
