use crate::core_ir::{CoreExpr, LiteralValue, MatchArm};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
}

impl ParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn parse_expr(input: &str) -> Result<CoreExpr, ParseError> {
    let mut parser = Parser { input, pos: 0 };
    let expr = parser.parse_expr()?;
    parser.skip_ws();
    if parser.pos != parser.input.len() {
        return Err(ParseError::new(format!(
            "unexpected trailing input at byte {}",
            parser.pos
        )));
    }
    Ok(expr)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn parse_expr(&mut self) -> Result<CoreExpr, ParseError> {
        self.skip_ws();
        if self.eof() {
            return Err(ParseError::new("expected expression"));
        }

        // String literal: "..."
        if self.peek() == Some('"') {
            let s = self.parse_string_literal()?;
            return Ok(CoreExpr::Literal(LiteralValue::Text(s)));
        }

        // Numeric literal: integer or float.
        // Float check: try parsing a number with an embedded '.'.
        if let Some(lit) = self.parse_number()? {
            return Ok(CoreExpr::Literal(lit));
        }

        let ident = self.parse_ident()?;
        self.skip_ws();
        if !self.consume('(') {
            return match ident.as_str() {
                "true" => Ok(CoreExpr::Literal(LiteralValue::Bool(true))),
                "false" => Ok(CoreExpr::Literal(LiteralValue::Bool(false))),
                _ => Ok(CoreExpr::Var(ident)),
            };
        }

        let args = self.parse_args()?;
        self.expr_from_call(ident, args)
    }

    /// Parse a quoted string literal `"..."`.
    ///
    /// Supports `\\` and `\"` escape sequences. Returns the unescaped content.
    fn parse_string_literal(&mut self) -> Result<String, ParseError> {
        // Consume opening `"`
        assert_eq!(self.peek(), Some('"'));
        self.pos += 1;
        let mut result = String::new();
        loop {
            match self.peek() {
                None => {
                    return Err(ParseError::new("unterminated string literal"));
                }
                Some('"') => {
                    self.pos += 1;
                    return Ok(result);
                }
                Some('\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some('"') => {
                            self.pos += 1;
                            result.push('"');
                        }
                        Some('\\') => {
                            self.pos += 1;
                            result.push('\\');
                        }
                        Some('n') => {
                            self.pos += 1;
                            result.push('\n');
                        }
                        Some('t') => {
                            self.pos += 1;
                            result.push('\t');
                        }
                        other => {
                            return Err(ParseError::new(format!(
                                "unsupported escape sequence: \\{:?}",
                                other
                            )));
                        }
                    }
                }
                Some(ch) => {
                    self.pos += ch.len_utf8();
                    result.push(ch);
                }
            }
        }
    }

    /// Parse a numeric literal — integer or float.
    ///
    /// Returns `None` if the current position is not at a numeric token.
    /// Tries float first (requires a `.` in the number); falls back to int.
    fn parse_number(&mut self) -> Result<Option<LiteralValue>, ParseError> {
        let start = self.pos;
        // Optional leading minus.
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        let digits_start = self.pos;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.pos += 1;
        }
        if digits_start == self.pos {
            // No digits consumed — not a number.
            self.pos = start;
            return Ok(None);
        }

        // Check for decimal point followed by at least one digit → float.
        if self.peek() == Some('.')
            && self.input[self.pos + 1..].starts_with(|ch: char| ch.is_ascii_digit())
        {
            self.pos += 1; // consume '.'
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.pos += 1;
            }
            let text = &self.input[start..self.pos];
            let value = text
                .parse::<f64>()
                .map_err(|_| ParseError::new(format!("float literal out of range: {text}")))?;
            return Ok(Some(LiteralValue::Float(value)));
        }

        // Pure integer.
        let text = &self.input[start..self.pos];
        let value = text
            .parse::<i64>()
            .map_err(|_| ParseError::new(format!("integer literal out of range: {text}")))?;
        Ok(Some(LiteralValue::Int(value)))
    }

    fn parse_args(&mut self) -> Result<Vec<CoreExpr>, ParseError> {
        let mut args = Vec::new();
        self.skip_ws();
        if self.consume(')') {
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr()?);
            self.skip_ws();
            if self.consume(')') {
                return Ok(args);
            }
            if !self.consume(',') {
                return Err(ParseError::new(format!(
                    "expected ',' or ')' at byte {}",
                    self.pos
                )));
            }
        }
    }

    fn expr_from_call(&self, func: String, args: Vec<CoreExpr>) -> Result<CoreExpr, ParseError> {
        match func.as_str() {
            "let" => {
                let [binding, value, body] = expect_arity::<3>(func, args)?;
                let CoreExpr::Var(name) = binding else {
                    return Err(ParseError::new("let binding name must be an identifier"));
                };
                Ok(CoreExpr::Let {
                    name,
                    value: Box::new(value),
                    body: Box::new(body),
                })
            }
            "if" => {
                let [cond, then_, else_] = expect_arity::<3>(func, args)?;
                Ok(CoreExpr::If {
                    cond: Box::new(cond),
                    then_: Box::new(then_),
                    else_: Box::new(else_),
                })
            }
            "match" => parse_match_call(args),
            "record" => parse_record_call(args),
            "field" => {
                let [record, field] = expect_arity::<2>(func, args)?;
                Ok(CoreExpr::FieldGet {
                    record: Box::new(record),
                    field: expect_name("field name", field)?,
                })
            }
            "update" => {
                let [record, field, value] = expect_arity::<3>(func, args)?;
                Ok(CoreExpr::FieldUpdate {
                    record: Box::new(record),
                    field: expect_name("field name", field)?,
                    value: Box::new(value),
                })
            }
            "tuple" => Ok(CoreExpr::TupleNew(args)),
            "variant" => parse_variant_call(args),
            "list" => Ok(CoreExpr::ListNew(args)),
            "add" => binary(func, args, CoreExpr::Add),
            "sub" => binary(func, args, CoreExpr::Sub),
            "mul" => binary(func, args, CoreExpr::Mul),
            "div" => binary(func, args, CoreExpr::Div),
            "mod" => binary(func, args, CoreExpr::Mod),
            "eq" => binary(func, args, CoreExpr::Eq),
            "ne" => binary(func, args, CoreExpr::Ne),
            "lt" => binary(func, args, CoreExpr::Lt),
            "le" => binary(func, args, CoreExpr::Le),
            "gt" => binary(func, args, CoreExpr::Gt),
            "ge" => binary(func, args, CoreExpr::Ge),
            "not" => {
                let [operand] = expect_arity::<1>(func, args)?;
                Ok(CoreExpr::Not(Box::new(operand)))
            }
            // Convenience constructors for Option/Result variants.
            "none" => {
                if !args.is_empty() {
                    return Err(ParseError::new(format!(
                        "none expects 0 args, got {}",
                        args.len()
                    )));
                }
                Ok(CoreExpr::VariantNew {
                    tag: "None".to_string(),
                    payload: None,
                })
            }
            "some" => {
                let [payload] = expect_arity::<1>(func, args)?;
                Ok(CoreExpr::VariantNew {
                    tag: "Some".to_string(),
                    payload: Some(Box::new(payload)),
                })
            }
            "ok" => {
                let [payload] = expect_arity::<1>(func, args)?;
                Ok(CoreExpr::VariantNew {
                    tag: "Ok".to_string(),
                    payload: Some(Box::new(payload)),
                })
            }
            "err" => {
                let [payload] = expect_arity::<1>(func, args)?;
                Ok(CoreExpr::VariantNew {
                    tag: "Err".to_string(),
                    payload: Some(Box::new(payload)),
                })
            }
            "and" => {
                let [left, right] = expect_arity::<2>(func, args)?;
                Ok(CoreExpr::And {
                    left: Box::new(left),
                    right: Box::new(right),
                })
            }
            "or" => {
                let [left, right] = expect_arity::<2>(func, args)?;
                Ok(CoreExpr::Or {
                    left: Box::new(left),
                    right: Box::new(right),
                })
            }
            _ => Ok(CoreExpr::Call { func, args }),
        }
    }

    fn parse_ident(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        while self.peek().is_some_and(is_ident_char) {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(ParseError::new(format!(
                "expected identifier at byte {}",
                self.pos
            )));
        }
        Ok(self.input[start..self.pos].to_string())
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn eof(&self) -> bool {
        self.pos >= self.input.len()
    }
}

fn parse_match_call(args: Vec<CoreExpr>) -> Result<CoreExpr, ParseError> {
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

fn parse_record_call(args: Vec<CoreExpr>) -> Result<CoreExpr, ParseError> {
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

fn parse_variant_call(args: Vec<CoreExpr>) -> Result<CoreExpr, ParseError> {
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

fn expect_name(context: &str, expr: CoreExpr) -> Result<String, ParseError> {
    match expr {
        CoreExpr::Var(name) => Ok(name),
        _ => Err(ParseError::new(format!("{context} must be an identifier"))),
    }
}

fn render_match_pattern(pattern: CoreExpr) -> Result<String, ParseError> {
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
        _ => Err(ParseError::new(
            "match pattern must be an identifier, literal, wildcard, or constructor pattern",
        )),
    }
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.')
}

fn binary(
    func: String,
    args: Vec<CoreExpr>,
    make: fn(Box<CoreExpr>, Box<CoreExpr>) -> CoreExpr,
) -> Result<CoreExpr, ParseError> {
    let [left, right] = expect_arity::<2>(func, args)?;
    Ok(make(Box::new(left), Box::new(right)))
}

fn expect_arity<const N: usize>(
    func: String,
    args: Vec<CoreExpr>,
) -> Result<[CoreExpr; N], ParseError> {
    let actual = args.len();
    args.try_into()
        .map_err(|_| ParseError::new(format!("{func} expects {N} args, got {actual}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_add_call_to_core_expr_add() {
        assert_eq!(
            parse_expr("add(x, y)").unwrap(),
            CoreExpr::Add(
                Box::new(CoreExpr::Var("x".to_string())),
                Box::new(CoreExpr::Var("y".to_string()))
            )
        );
    }

    #[test]
    fn parses_nested_sum_of_squares() {
        assert_eq!(
            parse_expr("add(mul(x, x), mul(y, y))").unwrap(),
            CoreExpr::Add(
                Box::new(CoreExpr::Mul(
                    Box::new(CoreExpr::Var("x".to_string())),
                    Box::new(CoreExpr::Var("x".to_string()))
                )),
                Box::new(CoreExpr::Mul(
                    Box::new(CoreExpr::Var("y".to_string())),
                    Box::new(CoreExpr::Var("y".to_string()))
                ))
            )
        );
    }

    #[test]
    fn parses_let_binding() {
        assert_eq!(
            parse_expr("let(total, add(x, y), if(gt(total, 10), total, 0))").unwrap(),
            CoreExpr::Let {
                name: "total".to_string(),
                value: Box::new(CoreExpr::Add(
                    Box::new(CoreExpr::Var("x".to_string())),
                    Box::new(CoreExpr::Var("y".to_string()))
                )),
                body: Box::new(CoreExpr::If {
                    cond: Box::new(CoreExpr::Gt(
                        Box::new(CoreExpr::Var("total".to_string())),
                        Box::new(CoreExpr::Literal(LiteralValue::Int(10)))
                    )),
                    then_: Box::new(CoreExpr::Var("total".to_string())),
                    else_: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
                }),
            }
        );
    }

    #[test]
    fn rejects_non_identifier_let_binding() {
        let err = parse_expr("let(add(x, y), 1, 2)").unwrap_err();
        assert_eq!(err.message, "let binding name must be an identifier");
    }

    #[test]
    fn parses_short_circuit_boolean_forms() {
        assert_eq!(
            parse_expr("and(flag, gt(total, 0))").unwrap(),
            CoreExpr::And {
                left: Box::new(CoreExpr::Var("flag".to_string())),
                right: Box::new(CoreExpr::Gt(
                    Box::new(CoreExpr::Var("total".to_string())),
                    Box::new(CoreExpr::Literal(LiteralValue::Int(0)))
                )),
            }
        );
        assert_eq!(
            parse_expr("or(flag, eq(total, 0))").unwrap(),
            CoreExpr::Or {
                left: Box::new(CoreExpr::Var("flag".to_string())),
                right: Box::new(CoreExpr::Eq(
                    Box::new(CoreExpr::Var("total".to_string())),
                    Box::new(CoreExpr::Literal(LiteralValue::Int(0)))
                )),
            }
        );
    }

    #[test]
    fn parses_match_expression() {
        assert_eq!(
            parse_expr("match(score, 1, 10, 2, 20, _, 0)").unwrap(),
            CoreExpr::Match {
                scrutinee: Box::new(CoreExpr::Var("score".to_string())),
                arms: vec![
                    MatchArm {
                        pattern: "1".to_string(),
                        body: CoreExpr::Literal(LiteralValue::Int(10)),
                    },
                    MatchArm {
                        pattern: "2".to_string(),
                        body: CoreExpr::Literal(LiteralValue::Int(20)),
                    },
                    MatchArm {
                        pattern: "_".to_string(),
                        body: CoreExpr::Literal(LiteralValue::Int(0)),
                    },
                ],
            }
        );
    }

    #[test]
    fn parses_match_constructor_pattern() {
        assert_eq!(
            parse_expr("match(result, Ok(value), value, _, 0)").unwrap(),
            CoreExpr::Match {
                scrutinee: Box::new(CoreExpr::Var("result".to_string())),
                arms: vec![
                    MatchArm {
                        pattern: "Ok(value)".to_string(),
                        body: CoreExpr::Var("value".to_string()),
                    },
                    MatchArm {
                        pattern: "_".to_string(),
                        body: CoreExpr::Literal(LiteralValue::Int(0)),
                    },
                ],
            }
        );
    }

    #[test]
    fn parses_compound_value_forms() {
        assert_eq!(
            parse_expr("record(age, 30, score, add(10, 5))").unwrap(),
            CoreExpr::RecordNew {
                fields: vec![
                    ("age".to_string(), CoreExpr::Literal(LiteralValue::Int(30))),
                    (
                        "score".to_string(),
                        CoreExpr::Add(
                            Box::new(CoreExpr::Literal(LiteralValue::Int(10))),
                            Box::new(CoreExpr::Literal(LiteralValue::Int(5))),
                        ),
                    ),
                ],
            }
        );

        assert_eq!(
            parse_expr("field(person, age)").unwrap(),
            CoreExpr::FieldGet {
                record: Box::new(CoreExpr::Var("person".to_string())),
                field: "age".to_string(),
            }
        );

        assert_eq!(
            parse_expr("variant(Some, 7)").unwrap(),
            CoreExpr::VariantNew {
                tag: "Some".to_string(),
                payload: Some(Box::new(CoreExpr::Literal(LiteralValue::Int(7)))),
            }
        );

        assert_eq!(
            parse_expr("list(1, 2, 3)").unwrap(),
            CoreExpr::ListNew(vec![
                CoreExpr::Literal(LiteralValue::Int(1)),
                CoreExpr::Literal(LiteralValue::Int(2)),
                CoreExpr::Literal(LiteralValue::Int(3)),
            ])
        );
    }

    #[test]
    fn rejects_malformed_compound_value_forms() {
        let err = parse_expr("record(age, 30, dangling)").unwrap_err();
        assert_eq!(err.message, "record expects field/value pairs, got 3 args");

        let err = parse_expr("field(person, 1)").unwrap_err();
        assert_eq!(err.message, "field name must be an identifier");

        let err = parse_expr("variant(Some, 1, 2)").unwrap_err();
        assert_eq!(err.message, "variant expects 1 or 2 args, got 3");
    }

    #[test]
    fn rejects_match_without_pattern_body_pairs() {
        let err = parse_expr("match(value, 1)").unwrap_err();
        assert_eq!(
            err.message,
            "match expects scrutinee plus pattern/body pairs, got 2 args"
        );
    }

    // ── New comparison and boolean operators ─────────────────────────────

    #[test]
    fn parses_ne_le_ge_comparison_operators() {
        assert_eq!(
            parse_expr("ne(x, y)").unwrap(),
            CoreExpr::Ne(
                Box::new(CoreExpr::Var("x".to_string())),
                Box::new(CoreExpr::Var("y".to_string()))
            )
        );
        assert_eq!(
            parse_expr("le(x, 10)").unwrap(),
            CoreExpr::Le(
                Box::new(CoreExpr::Var("x".to_string())),
                Box::new(CoreExpr::Literal(LiteralValue::Int(10)))
            )
        );
        assert_eq!(
            parse_expr("ge(score, 0)").unwrap(),
            CoreExpr::Ge(
                Box::new(CoreExpr::Var("score".to_string())),
                Box::new(CoreExpr::Literal(LiteralValue::Int(0)))
            )
        );
    }

    #[test]
    fn parses_not_operator() {
        assert_eq!(
            parse_expr("not(flag)").unwrap(),
            CoreExpr::Not(Box::new(CoreExpr::Var("flag".to_string())))
        );
        // not applied to a comparison
        assert_eq!(
            parse_expr("not(eq(x, 0))").unwrap(),
            CoreExpr::Not(Box::new(CoreExpr::Eq(
                Box::new(CoreExpr::Var("x".to_string())),
                Box::new(CoreExpr::Literal(LiteralValue::Int(0)))
            )))
        );
    }

    // ── Float and string literals ────────────────────────────────────────

    #[test]
    fn parses_float_literals() {
        match parse_expr("3.14").unwrap() {
            CoreExpr::Literal(LiteralValue::Float(f)) => {
                assert!((f - 3.14).abs() < 1e-10, "expected 3.14, got {f}");
            }
            other => panic!("expected Float literal, got {other:?}"),
        }
        match parse_expr("-2.5").unwrap() {
            CoreExpr::Literal(LiteralValue::Float(f)) => {
                assert!((f - (-2.5)).abs() < 1e-10, "expected -2.5, got {f}");
            }
            other => panic!("expected Float literal, got {other:?}"),
        }
    }

    #[test]
    fn parses_string_literals() {
        assert_eq!(
            parse_expr("\"hello\"").unwrap(),
            CoreExpr::Literal(LiteralValue::Text("hello".to_string()))
        );
        assert_eq!(
            parse_expr("\"hello world\"").unwrap(),
            CoreExpr::Literal(LiteralValue::Text("hello world".to_string()))
        );
        // Escape sequences
        assert_eq!(
            parse_expr("\"say \\\"hi\\\"\"").unwrap(),
            CoreExpr::Literal(LiteralValue::Text("say \"hi\"".to_string()))
        );
        assert_eq!(
            parse_expr("\"line\\nnewline\"").unwrap(),
            CoreExpr::Literal(LiteralValue::Text("line\nnewline".to_string()))
        );
    }

    #[test]
    fn rejects_unterminated_string_literal() {
        let err = parse_expr("\"unterminated").unwrap_err();
        assert_eq!(err.message, "unterminated string literal");
    }

    // ── Option/Result convenience constructors ───────────────────────────

    #[test]
    fn parses_option_result_convenience_constructors() {
        assert_eq!(
            parse_expr("none()").unwrap(),
            CoreExpr::VariantNew {
                tag: "None".to_string(),
                payload: None,
            }
        );
        assert_eq!(
            parse_expr("some(42)").unwrap(),
            CoreExpr::VariantNew {
                tag: "Some".to_string(),
                payload: Some(Box::new(CoreExpr::Literal(LiteralValue::Int(42)))),
            }
        );
        assert_eq!(
            parse_expr("ok(x)").unwrap(),
            CoreExpr::VariantNew {
                tag: "Ok".to_string(),
                payload: Some(Box::new(CoreExpr::Var("x".to_string()))),
            }
        );
        assert_eq!(
            parse_expr("err(msg)").unwrap(),
            CoreExpr::VariantNew {
                tag: "Err".to_string(),
                payload: Some(Box::new(CoreExpr::Var("msg".to_string()))),
            }
        );
    }

    #[test]
    fn rejects_none_with_arguments() {
        let err = parse_expr("none(x)").unwrap_err();
        assert_eq!(err.message, "none expects 0 args, got 1");
    }

    // ── Match with Option/Result constructor patterns ─────────────────────

    #[test]
    fn parses_match_with_option_constructor_patterns() {
        // match(opt, Some(v), v, None, 0)
        assert_eq!(
            parse_expr("match(opt, Some(v), v, None, 0)").unwrap(),
            CoreExpr::Match {
                scrutinee: Box::new(CoreExpr::Var("opt".to_string())),
                arms: vec![
                    MatchArm {
                        pattern: "Some(v)".to_string(),
                        body: CoreExpr::Var("v".to_string()),
                    },
                    MatchArm {
                        pattern: "None".to_string(),
                        body: CoreExpr::Literal(LiteralValue::Int(0)),
                    },
                ],
            }
        );
    }

    #[test]
    fn parses_match_with_result_constructor_patterns() {
        // match(result, Ok(val), val, Err(e), -1)
        assert_eq!(
            parse_expr("match(result, Ok(val), val, Err(e), -1)").unwrap(),
            CoreExpr::Match {
                scrutinee: Box::new(CoreExpr::Var("result".to_string())),
                arms: vec![
                    MatchArm {
                        pattern: "Ok(val)".to_string(),
                        body: CoreExpr::Var("val".to_string()),
                    },
                    MatchArm {
                        pattern: "Err(e)".to_string(),
                        body: CoreExpr::Literal(LiteralValue::Int(-1)),
                    },
                ],
            }
        );
    }

    // ── Nested expressions with new operators ────────────────────────────

    #[test]
    fn parses_nested_range_check_with_le_ge() {
        // and(ge(x, 0), le(x, 100))  — checks 0 <= x <= 100
        assert_eq!(
            parse_expr("and(ge(x, 0), le(x, 100))").unwrap(),
            CoreExpr::And {
                left: Box::new(CoreExpr::Ge(
                    Box::new(CoreExpr::Var("x".to_string())),
                    Box::new(CoreExpr::Literal(LiteralValue::Int(0)))
                )),
                right: Box::new(CoreExpr::Le(
                    Box::new(CoreExpr::Var("x".to_string())),
                    Box::new(CoreExpr::Literal(LiteralValue::Int(100)))
                )),
            }
        );
    }
}
