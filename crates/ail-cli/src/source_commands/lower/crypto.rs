use super::*;

pub(super) fn lower_source_crypto_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((lowered_func, arity, usage)) = source_crypto_helper_lowering(&func) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::CryptoHelper,
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

fn source_crypto_helper_lowering(func: &str) -> Option<(&'static str, usize, &'static str)> {
    match func {
        "crypto.hash" | "crypto_hash" | "std.crypto.hash" => {
            Some(("std.crypto.hash", 1, "crypto_hash(bytes)"))
        }
        "crypto.hmac" | "crypto_hmac" | "std.crypto.hmac" => {
            Some(("std.crypto.hmac", 2, "crypto_hmac(key, message)"))
        }
        "crypto.constant_time_eq" | "crypto_constant_time_eq" | "std.crypto.constant_time_eq" => {
            Some((
                "std.crypto.constant_time_eq",
                2,
                "crypto_constant_time_eq(left, right)",
            ))
        }
        _ => None,
    }
}
