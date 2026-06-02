use super::*;

pub(super) fn lower_source_env_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((capability, operation, arity, usage)) = source_env_helper_lowering(&func) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::EnvHelper,
            format!("{func} requires `{usage}`"),
        ));
    }
    let mut lowered = vec![capability.to_string(), operation.to_string()];
    lowered.extend(
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(Some(format!("effect_call({})", lowered.join(", "))))
}

fn source_env_helper_lowering(
    func: &str,
) -> Option<(&'static str, &'static str, usize, &'static str)> {
    match func {
        "env.get" | "env_get" | "std.env.get" => Some(("env.read", "get", 1, "env_get(key)")),
        "env.set" | "env_set" | "std.env.set" => {
            Some(("env.write", "set", 2, "env_set(key, value)"))
        }
        "env.list" | "env_list" | "std.env.list" => Some(("env.read", "list", 0, "env_list()")),
        _ => None,
    }
}
