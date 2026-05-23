use crate::core_ir::{CoreExpr, LiteralValue};

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

        if let Some(n) = self.parse_int()? {
            return Ok(CoreExpr::Literal(LiteralValue::Int(n)));
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
            "add" => binary(func, args, CoreExpr::Add),
            "sub" => binary(func, args, CoreExpr::Sub),
            "mul" => binary(func, args, CoreExpr::Mul),
            "div" => binary(func, args, CoreExpr::Div),
            "mod" => binary(func, args, CoreExpr::Mod),
            "eq" => binary(func, args, CoreExpr::Eq),
            "lt" => binary(func, args, CoreExpr::Lt),
            "gt" => binary(func, args, CoreExpr::Gt),
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

    fn parse_int(&mut self) -> Result<Option<i64>, ParseError> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        let digits_start = self.pos;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.pos += 1;
        }
        if digits_start == self.pos {
            self.pos = start;
            return Ok(None);
        }
        self.input[start..self.pos]
            .parse::<i64>()
            .map(Some)
            .map_err(|_| ParseError::new("integer literal is out of range"))
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
}
