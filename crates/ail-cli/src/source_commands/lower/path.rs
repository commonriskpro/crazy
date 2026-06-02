use super::*;

pub(super) fn lower_source_path_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some(usage) = source_path_helper_usage(&func) else {
        return Ok(None);
    };
    if args.len() != 1 {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::PathHelper,
            format!("{func} requires `{usage}`"),
        ));
    }
    let lowered_arg = lower_source_expr(&args[0], line_num)?;
    let helper = match func.as_str() {
        "path.from_text" | "path_from_text" | "std.path.from_text" => "std.path.from_text",
        "path.to_text" | "path_to_text" | "std.path.to_text" => "std.path.to_text",
        _ => unreachable!("checked source path helper"),
    };
    Ok(Some(format!("{helper}({lowered_arg})")))
}

fn source_path_helper_usage(func: &str) -> Option<&'static str> {
    match func {
        "path.from_text" | "path_from_text" | "std.path.from_text" => Some("path_from_text(text)"),
        "path.to_text" | "path_to_text" | "std.path.to_text" => Some("path_to_text(path)"),
        _ => None,
    }
}
