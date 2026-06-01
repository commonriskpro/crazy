use super::*;

pub(super) fn lower_source_encoding_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((lowered_func, arity, usage)) = source_encoding_helper_lowering(&func) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::EncodingHelper,
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

fn source_encoding_helper_lowering(func: &str) -> Option<(&'static str, usize, &'static str)> {
    match func {
        "encoding.base64_encode" | "encoding_base64_encode" | "std.encoding.base64_encode" => {
            Some((
                "std.encoding.base64_encode",
                1,
                "encoding_base64_encode(bytes)",
            ))
        }
        "encoding.base64_decode" | "encoding_base64_decode" | "std.encoding.base64_decode" => {
            Some((
                "std.encoding.base64_decode",
                1,
                "encoding_base64_decode(text)",
            ))
        }
        "encoding.hex_encode" | "encoding_hex_encode" | "std.encoding.hex_encode" => {
            Some(("std.encoding.hex_encode", 1, "encoding_hex_encode(bytes)"))
        }
        "encoding.hex_decode" | "encoding_hex_decode" | "std.encoding.hex_decode" => {
            Some(("std.encoding.hex_decode", 1, "encoding_hex_decode(text)"))
        }
        _ => None,
    }
}
