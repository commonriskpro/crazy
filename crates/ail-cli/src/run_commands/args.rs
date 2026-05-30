use super::*;

// ── Private helpers ───────────────────────────────────────────────────────

pub(crate) fn parse_runtime_args(args: &[String]) -> Result<Vec<RuntimeArg>, CliError> {
    args.iter()
        .map(|arg| {
            if let Some(rest) = arg.strip_prefix("i32:") {
                rest.parse::<i32>().map(RuntimeArg::I32).map_err(|_| {
                    CliError::ParseError(format!("run argument '{arg}' has invalid i32 value"))
                })
            } else if let Some(rest) = arg.strip_prefix("f64:") {
                rest.parse::<f64>().map(RuntimeArg::F64).map_err(|_| {
                    CliError::ParseError(format!("run argument '{arg}' has invalid f64 value"))
                })
            } else {
                arg.parse::<i64>().map(RuntimeArg::I64).map_err(|_| {
                    CliError::ParseError(format!(
                        "run argument '{arg}' is not an integer \
                        (use i32:<n> or f64:<n> for typed args)"
                    ))
                })
            }
        })
        .collect()
}
