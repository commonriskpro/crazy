// ── ail-compiler::expr_parser — free helper functions ─────────────────────
//
// Declared from expr_parser.rs as:
//   #[path = "expr_parser_helpers.rs"]
//   mod helpers;

use crate::core_ir::{CoreExpr, LiteralValue, MatchArm};

use super::ParseError;

pub(crate) fn parse_match_call(args: Vec<CoreExpr>) -> Result<CoreExpr, ParseError> {
    if args.len() < 3 || args.len().is_multiple_of(2) {
        return Err(ParseError::new(format!(
            "match expects scrutinee plus pattern/body pairs, got {} args",
            args.len()
        )));
    }

    let mut args = args.into_iter();
    let scrutinee = args.next().expect("len checked above");
    let mut arms = Vec::new();
    while let Some(pattern) = args.next() {
        let body = args.next().expect("odd arg count checked above");
        arms.push(MatchArm {
            pattern: render_match_pattern(pattern)?,
            body,
        });
    }

    Ok(CoreExpr::Match {
        scrutinee: Box::new(scrutinee),
        arms,
    })
}

pub(crate) fn parse_record_call(args: Vec<CoreExpr>) -> Result<CoreExpr, ParseError> {
    if !args.len().is_multiple_of(2) {
        return Err(ParseError::new(format!(
            "record expects field/value pairs, got {} args",
            args.len()
        )));
    }

    let mut args = args.into_iter();
    let mut fields = Vec::new();
    while let Some(field) = args.next() {
        let value = args.next().expect("even arg count checked above");
        fields.push((expect_name("record field name", field)?, value));
    }

    Ok(CoreExpr::RecordNew { fields })
}

pub(crate) fn parse_variant_call(args: Vec<CoreExpr>) -> Result<CoreExpr, ParseError> {
    match args.len() {
        1 | 2 => {
            let mut args = args.into_iter();
            let tag = expect_name("variant tag", args.next().expect("len checked above"))?;
            let payload = args.next().map(Box::new);
            Ok(CoreExpr::VariantNew { tag, payload })
        }
        actual => Err(ParseError::new(format!(
            "variant expects 1 or 2 args, got {actual}"
        ))),
    }
}

pub(crate) fn expect_name(context: &str, expr: CoreExpr) -> Result<String, ParseError> {
    match expr {
        CoreExpr::Var(name) => Ok(name),
        _ => Err(ParseError::new(format!("{context} must be an identifier"))),
    }
}

pub(crate) fn render_match_pattern(pattern: CoreExpr) -> Result<String, ParseError> {
    match pattern {
        CoreExpr::Var(name) => Ok(name),
        CoreExpr::Literal(LiteralValue::Bool(value)) => Ok(value.to_string()),
        CoreExpr::Literal(LiteralValue::Int(value)) => Ok(value.to_string()),
        CoreExpr::Call { func, args } => {
            let rendered_args = args
                .into_iter()
                .map(render_match_pattern)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("{func}({})", rendered_args.join(", ")))
        }
        // `(variant "Tag" payload)` desugars to VariantNew at parse time;
        // when it appears in a match-pattern position it must render as the
        // canonical constructor-pattern string the WASM backend expects.
        CoreExpr::VariantNew { tag, payload } => match payload {
            None => Ok(tag),
            Some(inner) => {
                let rendered = render_match_pattern(*inner)?;
                Ok(format!("{tag}({rendered})"))
            }
        },
        _ => Err(ParseError::new(
            "match pattern must be an identifier, literal, wildcard, or constructor pattern",
        )),
    }
}

pub(crate) fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.')
}

pub(crate) fn binary(
    func: String,
    args: Vec<CoreExpr>,
    make: fn(Box<CoreExpr>, Box<CoreExpr>) -> CoreExpr,
) -> Result<CoreExpr, ParseError> {
    let [left, right] = expect_arity::<2>(func, args)?;
    Ok(make(Box::new(left), Box::new(right)))
}

pub(crate) fn expect_arity<const N: usize>(
    func: String,
    args: Vec<CoreExpr>,
) -> Result<[CoreExpr; N], ParseError> {
    let actual = args.len();
    args.try_into()
        .map_err(|_| ParseError::new(format!("{func} expects {N} args, got {actual}")))
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::core_ir::{CoreExpr, LiteralValue};

    use super::render_match_pattern;

    #[test]
    fn rmp_variant_tag_only() {
        // `(variant "None")` → no payload → renders as the bare tag string.
        let expr = CoreExpr::VariantNew {
            tag: "None".to_string(),
            payload: None,
        };
        assert_eq!(render_match_pattern(expr).unwrap(), "None");
    }

    #[test]
    fn rmp_variant_with_var_payload() {
        // `(variant "Some" x)` → renders as `Some(x)` — canonical constructor
        // pattern string expected by the WASM backend.
        let expr = CoreExpr::VariantNew {
            tag: "Some".to_string(),
            payload: Some(Box::new(CoreExpr::Var("x".to_string()))),
        };
        assert_eq!(render_match_pattern(expr).unwrap(), "Some(x)");
    }

    #[test]
    fn rmp_variant_with_literal_payload() {
        // Payload may itself be a literal; render_match_pattern recurses.
        let expr = CoreExpr::VariantNew {
            tag: "Ok".to_string(),
            payload: Some(Box::new(CoreExpr::Literal(LiteralValue::Int(42)))),
        };
        assert_eq!(render_match_pattern(expr).unwrap(), "Ok(42)");
    }
}
