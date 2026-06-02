use super::*;

pub(super) fn lower_source_random_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((capability, operation, arity, usage)) = source_random_helper_lowering(&func) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::RandomHelper,
            format!("{func} requires `{usage}`"),
        ));
    }
    Ok(Some(format!("effect_call({capability}, {operation})")))
}

fn source_random_helper_lowering(
    func: &str,
) -> Option<(&'static str, &'static str, usize, &'static str)> {
    match func {
        "random.next_int" | "random_next_int" | "std.random.next_int" => {
            Some(("random.int", "next_int", 0, "random_next_int()"))
        }
        _ => None,
    }
}
