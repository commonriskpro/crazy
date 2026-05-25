use crate::core_ir::{CoreExpr, LiteralValue};

#[path = "expr_parser_helpers.rs"]
mod helpers;
use helpers::{
    binary, expect_arity, expect_name, is_ident_char, parse_match_call, parse_record_call,
    parse_variant_call,
};

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
            // ── Control flow ─────────────────────────────────────────────
            //
            // `loop(body)` — infinite loop; exits via `break(value)`.
            "loop" => {
                let [body] = expect_arity::<1>(func, args)?;
                Ok(CoreExpr::Loop {
                    body: Box::new(body),
                    termination: None,
                })
            }
            // `while(cond, body)` — structured while loop.
            "while" => {
                let [cond, body] = expect_arity::<2>(func, args)?;
                Ok(CoreExpr::WhileLoop {
                    cond: Box::new(cond),
                    body: Box::new(body),
                    termination: None,
                })
            }
            // `break(value)` — exit the nearest enclosing loop.
            "break" => {
                let [value] = expect_arity::<1>(func, args)?;
                Ok(CoreExpr::Break {
                    value: Box::new(value),
                })
            }
            // `continue()` — restart the nearest enclosing loop.
            "continue" => {
                if !args.is_empty() {
                    return Err(ParseError::new(format!(
                        "continue expects 0 args, got {}",
                        args.len()
                    )));
                }
                Ok(CoreExpr::Continue)
            }
            // `return(value)` — explicit early return.
            "return" => {
                let [value] = expect_arity::<1>(func, args)?;
                Ok(CoreExpr::Return {
                    value: Box::new(value),
                })
            }
            // ── Effects ──────────────────────────────────────────────────
            //
            // `effect_call(capability, operation, arg1, arg2, ...)`
            //
            // `capability` and `operation` must be identifiers.
            // Remaining args are the call arguments.
            "effect_call" => {
                if args.len() < 2 {
                    return Err(ParseError::new(format!(
                        "effect_call expects at least 2 args (capability, operation), got {}",
                        args.len()
                    )));
                }
                let mut args = args.into_iter();
                let capability = expect_name("capability name", args.next().expect("len checked"))?;
                let op = expect_name("operation name", args.next().expect("len checked"))?;
                let call_args: Vec<CoreExpr> = args.collect();
                Ok(CoreExpr::EffectCall {
                    capability,
                    func: op,
                    args: call_args,
                })
            }
            // ── Lambda ───────────────────────────────────────────────────
            //
            // `lambda(param1, param2, ..., body)` — anonymous function.
            //
            // Convention: all but the last argument are parameter names
            // (must be identifiers); the last argument is the body expression.
            // `lambda(body)` creates a zero-parameter lambda.
            "lambda" => {
                if args.is_empty() {
                    return Err(ParseError::new(
                        "lambda expects at least 1 arg (body expression)",
                    ));
                }
                let mut all = args;
                let body = all.pop().expect("len checked above");
                let mut params = Vec::with_capacity(all.len());
                for param_expr in all {
                    params.push(expect_name("lambda parameter", param_expr)?);
                }
                Ok(CoreExpr::Lambda {
                    params,
                    body: Box::new(body),
                })
            }
            // ── Collection iteration ─────────────────────────────────────
            //
            // `foreach(binding, collection, body)` — structured iteration.
            // Parses correctly; WASM emit is a stub (trap) pending lowering.
            "foreach" => {
                let [binding, collection, body] = expect_arity::<3>(func, args)?;
                Ok(CoreExpr::ForEach {
                    binding: expect_name("foreach binding", binding)?,
                    collection: Box::new(collection),
                    body: Box::new(body),
                })
            }
            // `fold(init, list, func)` — left fold over a collection.
            // Parses correctly; WASM emit is a stub (trap) pending lowering.
            "fold" => {
                let [init, list, f] = expect_arity::<3>(func, args)?;
                Ok(CoreExpr::Fold {
                    init: Box::new(init),
                    list: Box::new(list),
                    func: Box::new(f),
                })
            }
            // ── Mutable cells ────────────────────────────────────────────
            //
            // `cell_new(init)` — create a mutable cell.
            "cell_new" => {
                let [init] = expect_arity::<1>(func, args)?;
                Ok(CoreExpr::CellNew {
                    init: Box::new(init),
                })
            }
            // `cell_get(cell)` — read the current value of a cell.
            "cell_get" => {
                let [cell] = expect_arity::<1>(func, args)?;
                Ok(CoreExpr::CellGet {
                    cell: Box::new(cell),
                })
            }
            // `cell_set(cell, value)` — write a new value into a cell.
            "cell_set" => {
                let [cell, value] = expect_arity::<2>(func, args)?;
                Ok(CoreExpr::CellSet {
                    cell: Box::new(cell),
                    value: Box::new(value),
                })
            }
            // ── Map and Set constructors ──────────────────────────────────
            //
            // `map(k1, v1, k2, v2, ...)` — construct a map from key/value pairs.
            //
            // Must have an even number of arguments (including zero).
            // Error on odd arity: keys without matching values are rejected.
            "map" => {
                if !args.len().is_multiple_of(2) {
                    return Err(ParseError::new(format!(
                        "map expects an even number of args (key/value pairs), got {}",
                        args.len()
                    )));
                }
                let mut entries = Vec::with_capacity(args.len() / 2);
                let mut iter = args.into_iter();
                while let Some(k) = iter.next() {
                    let v = iter.next().expect("even count checked above");
                    entries.push((k, v));
                }
                Ok(CoreExpr::MapNew { entries })
            }
            // `set(e1, e2, ...)` — construct a set from element expressions.
            //
            // Any arity is accepted, including zero (empty set).
            "set" => Ok(CoreExpr::SetNew { elements: args }),
            // `index(collection, index)` — read an element from a list by
            // zero-based integer index.
            //
            // Exactly 2 arguments required: the collection expression and the
            // index expression.  Wrong arity is a parse error.
            "index" => {
                let [collection, index] = expect_arity::<2>(func, args)?;
                Ok(CoreExpr::IndexGet {
                    collection: Box::new(collection),
                    index: Box::new(index),
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

#[cfg(test)]
#[path = "expr_parser_tests.rs"]
mod tests;
