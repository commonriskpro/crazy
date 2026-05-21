// ── ail-verify::z3_solver ─────────────────────────────────────────────────
//
// Z3-backed `Solver` implementation.
//
// # Z3 0.20.0 API notes
//
// z3 0.20.0 uses a **thread-local context** model — `Context::new()` is not
// public. All Z3 AST construction operates on the thread-local context, set
// by `with_z3_config(cfg, closure)`.  The closure runs with the provided
// `Config` (timeout, etc.) installed as the thread-local context, then
// restores the previous context on exit.
//
// # Design decisions (from spec ZSB / PP)
//
// - Tautology check: assert the *negation*, run `solver.check()`.
//   `Unsat`   → predicate is a tautology → `Proven`
//   `Sat`     → predicate is NOT a tautology → `Unsupported`
//   `Unknown` → timeout / resource limit → `Unsupported`
// - Parse error → `Unsupported` with reason.
// - Variables are modelled as `Int` constants (sufficient for contract
//   predicates; avoids a full type-inference pass).
// - Timeout set via `Config::set_param_value("timeout", "N")`.
//
// # Supported predicate grammar
//
// ```text
// expr         ::= or_expr
// or_expr      ::= and_expr ('||' and_expr)*
// and_expr     ::= not_expr ('&&' not_expr)*
// not_expr     ::= '!' not_expr | cmp_expr
// cmp_expr     ::= add_expr (CMP_OP add_expr)?
// add_expr     ::= mul_expr (('+' | '-') mul_expr)*
// mul_expr     ::= unary   (('*' | '/') unary)*
// unary        ::= '-' unary | atom
// atom         ::= '(' expr ')' | INT | IDENT | 'true' | 'false'
// CMP_OP       ::= '>' | '>=' | '<' | '<=' | '==' | '!='
// ```
//
// Anything outside this grammar returns `Unsupported`.

use z3::{Config, SatResult, Solver as Z3SolverInner, ast, with_z3_config};

use crate::proof::ProofObligation;
use crate::solver::{Solver, SolverOutcome};

// ── Z3Solver ──────────────────────────────────────────────────────────────

/// SMT-backed solver using the Z3 theorem prover.
///
/// Evaluates predicate strings from `ProofObligation` by parsing them into
/// Z3 expressions and checking whether the predicate is a *tautology*
/// (i.e. true in all possible variable assignments).
///
/// # Timeout
///
/// The default timeout is 5 000 ms. Use [`Z3Solver::with_timeout_ms`] to
/// customise.  When Z3 exceeds the timeout, `solve` returns
/// `SolverOutcome::Unsupported`.
///
/// # Thread safety
///
/// `Z3Solver` is `Send + Sync`; it holds no Z3 context. Each `solve()` call
/// uses `with_z3_config` to install a fresh scoped context.
pub struct Z3Solver {
    timeout_ms: u64,
}

impl Z3Solver {
    /// Create a solver with the default 5 000 ms timeout.
    pub fn new() -> Self {
        Self { timeout_ms: 5_000 }
    }

    /// Create a solver with a custom timeout in milliseconds.
    pub fn with_timeout_ms(ms: u64) -> Self {
        Self { timeout_ms: ms }
    }
}

impl Default for Z3Solver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for Z3Solver {
    /// Evaluate `obligation.predicate` as an SMT formula.
    ///
    /// Returns:
    /// - `Proven`      — predicate is a tautology (valid in all interpretations).
    /// - `Unsupported` — predicate is not a tautology, parse failed, or timeout.
    fn solve(&self, obligation: &ProofObligation) -> SolverOutcome {
        let predicate = obligation.predicate.trim().to_string();
        let timeout_ms = self.timeout_ms;

        let mut cfg = Config::new();
        // Z3 timeout param is a string value in milliseconds.
        cfg.set_param_value("timeout", &timeout_ms.to_string());

        with_z3_config(&cfg, move || {
            match parse_bool(&predicate) {
                Err(_reason) => SolverOutcome::Unsupported,
                Ok(formula) => {
                    let solver = Z3SolverInner::new();
                    // Tautology check: ¬formula must be unsatisfiable.
                    solver.assert(formula.not());
                    match solver.check() {
                        SatResult::Unsat => SolverOutcome::Proven,
                        SatResult::Sat => SolverOutcome::Unsupported,
                        SatResult::Unknown => SolverOutcome::Unsupported,
                    }
                }
            }
        })
    }
}

// ── Token ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Int(i64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Gt,
    Ge,
    Lt,
    Le,
    Eq,
    Ne,
    And,
    Or,
    Not,
    LParen,
    RParen,
}

// ── Tokenizer ─────────────────────────────────────────────────────────────

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut pos = 0;
    let mut tokens = Vec::new();

    while pos < chars.len() {
        let ch = chars[pos];

        if ch.is_whitespace() {
            pos += 1;
            continue;
        }

        // Integer literal.
        if ch.is_ascii_digit() {
            let start = pos;
            while pos < chars.len() && chars[pos].is_ascii_digit() {
                pos += 1;
            }
            let s: String = chars[start..pos].iter().collect();
            let n: i64 = s.parse().map_err(|_| format!("integer overflow: {s}"))?;
            tokens.push(Token::Int(n));
            continue;
        }

        // Identifier / keyword.
        if ch.is_alphabetic() || ch == '_' {
            let start = pos;
            while pos < chars.len() && (chars[pos].is_alphanumeric() || chars[pos] == '_') {
                pos += 1;
            }
            let s: String = chars[start..pos].iter().collect();
            tokens.push(Token::Ident(s));
            continue;
        }

        // Two-character operators — check before single-char.
        if pos + 1 < chars.len() {
            let two: String = chars[pos..pos + 2].iter().collect();
            match two.as_str() {
                ">=" => {
                    tokens.push(Token::Ge);
                    pos += 2;
                    continue;
                }
                "<=" => {
                    tokens.push(Token::Le);
                    pos += 2;
                    continue;
                }
                "==" => {
                    tokens.push(Token::Eq);
                    pos += 2;
                    continue;
                }
                "!=" => {
                    tokens.push(Token::Ne);
                    pos += 2;
                    continue;
                }
                "&&" => {
                    tokens.push(Token::And);
                    pos += 2;
                    continue;
                }
                "||" => {
                    tokens.push(Token::Or);
                    pos += 2;
                    continue;
                }
                _ => {}
            }
        }

        // Single-character tokens.
        match ch {
            '>' => tokens.push(Token::Gt),
            '<' => tokens.push(Token::Lt),
            '+' => tokens.push(Token::Plus),
            '-' => tokens.push(Token::Minus),
            '*' => tokens.push(Token::Star),
            '/' => tokens.push(Token::Slash),
            '!' => tokens.push(Token::Not),
            '(' => tokens.push(Token::LParen),
            ')' => tokens.push(Token::RParen),
            other => return Err(format!("unexpected character: {other:?}")),
        }
        pos += 1;
    }

    Ok(tokens)
}

// ── Parser ────────────────────────────────────────────────────────────────
//
// Parses a `Token` stream into `ast::Bool` / `ast::Int` Z3 AST nodes.
// Uses the thread-local Z3 context (set by `with_z3_config`).

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Variables declared so far, keyed by name.
    vars: std::collections::HashMap<String, ast::Int>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            vars: std::collections::HashMap::new(),
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn consume(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(tok)
        } else {
            None
        }
    }

    fn expect_end(&self) -> Result<(), String> {
        if self.pos == self.tokens.len() {
            Ok(())
        } else {
            Err(format!(
                "unexpected token after expression: {:?}",
                self.tokens[self.pos]
            ))
        }
    }

    /// Return or create an `Int` Z3 variable for `name`.
    fn int_var(&mut self, name: &str) -> ast::Int {
        if !self.vars.contains_key(name) {
            let v = ast::Int::new_const(name);
            self.vars.insert(name.to_string(), v);
        }
        self.vars[name].clone()
    }

    // ── Grammar rules (descending precedence) ─────────────────────────────

    fn parse_expr(&mut self) -> Result<ast::Bool, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<ast::Bool, String> {
        let mut left = self.parse_and()?;
        while self.peek() == Some(&Token::Or) {
            self.consume();
            let right = self.parse_and()?;
            left = ast::Bool::or(&[&left, &right]);
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<ast::Bool, String> {
        let mut left = self.parse_not()?;
        while self.peek() == Some(&Token::And) {
            self.consume();
            let right = self.parse_not()?;
            left = ast::Bool::and(&[&left, &right]);
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<ast::Bool, String> {
        if self.peek() == Some(&Token::Not) {
            self.consume();
            // After `!`, check if next token is `(` — if so, parse the
            // parenthesised expression as a full boolean subexpression.
            if self.peek() == Some(&Token::LParen) {
                self.consume();
                let inner = self.parse_expr()?;
                match self.consume() {
                    Some(Token::RParen) => return Ok(inner.not()),
                    _ => return Err("expected ')' after '!('".to_string()),
                }
            }
            let inner = self.parse_not()?;
            return Ok(inner.not());
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<ast::Bool, String> {
        // Special case: bare boolean atoms (true / false keywords already
        // converted to Bool by fast path; identifiers can be boolean vars).
        // We try to parse as int first; if we can't get a comparison,
        // we must have a boolean atom directly.
        let left_int = self.parse_add()?;

        // Peek for comparison operator.
        let op = match self.peek() {
            Some(Token::Gt) => Token::Gt,
            Some(Token::Ge) => Token::Ge,
            Some(Token::Lt) => Token::Lt,
            Some(Token::Le) => Token::Le,
            Some(Token::Eq) => Token::Eq,
            Some(Token::Ne) => Token::Ne,
            _ => {
                return Err(
                    "expected comparison operator; bare integer variables are not boolean"
                        .to_string(),
                );
            }
        };
        self.consume();
        let right_int = self.parse_add()?;

        let result = match op {
            Token::Gt => left_int.gt(&right_int),
            Token::Ge => left_int.ge(&right_int),
            Token::Lt => left_int.lt(&right_int),
            Token::Le => left_int.le(&right_int),
            Token::Eq => left_int.eq(&right_int),
            Token::Ne => left_int.eq(&right_int).not(),
            _ => unreachable!(),
        };
        Ok(result)
    }

    fn parse_add(&mut self) -> Result<ast::Int, String> {
        let mut left = self.parse_mul()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.consume();
                    let right = self.parse_mul()?;
                    left += right;
                }
                Some(Token::Minus) => {
                    self.consume();
                    let right = self.parse_mul()?;
                    left -= right;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<ast::Int, String> {
        let mut left = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.consume();
                    let right = self.parse_unary()?;
                    left *= right;
                }
                Some(Token::Slash) => {
                    self.consume();
                    let right = self.parse_unary()?;
                    left = ast::Int::div(&left, &right);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<ast::Int, String> {
        if self.peek() == Some(&Token::Minus) {
            self.consume();
            let inner = self.parse_unary()?;
            return Ok(-inner);
        }
        self.parse_int_atom()
    }

    fn parse_int_atom(&mut self) -> Result<ast::Int, String> {
        match self.peek().cloned() {
            Some(Token::Int(n)) => {
                self.consume();
                Ok(ast::Int::from_i64(n))
            }
            Some(Token::Ident(name)) => {
                // Reject dot-notation (e.g. "user.age") — unsupported.
                if name.contains('.') {
                    return Err(format!("dot-notation not supported: {name}"));
                }
                self.consume();
                Ok(self.int_var(&name))
            }
            Some(Token::LParen) => {
                self.consume();
                // Parenthesised expression — could be bool or int.
                // We need to decide based on context; for arithmetic contexts
                // we require an int expression — reject bare boolean parens.
                let inner_int = self.parse_add()?;
                match self.consume() {
                    Some(Token::RParen) => Ok(inner_int),
                    _ => Err("expected ')'".to_string()),
                }
            }
            other => Err(format!("expected integer atom, got {other:?}")),
        }
    }
}

// ── Top-level parser entry point ──────────────────────────────────────────

/// Parse `input` into a `Bool` Z3 AST using the current thread-local context.
///
/// Returns `Err(reason)` if the input cannot be parsed.
fn parse_bool(input: &str) -> Result<ast::Bool, String> {
    // Fast path for boolean literals.
    match input.trim() {
        "true" => return Ok(ast::Bool::from_bool(true)),
        "false" => return Ok(ast::Bool::from_bool(false)),
        _ => {}
    }

    let tokens = tokenize(input)?;

    // Check for unsupported patterns: dot-notation, keywords in ident position.
    // (dot detection also happens in parse_int_atom, but catch early for clarity.)
    for tok in &tokens {
        if let Token::Ident(name) = tok
            && name.contains('.')
        {
            return Err(format!("dot-notation not supported: {name}"));
        }
    }

    let mut parser = Parser::new(tokens);
    let result = parser.parse_expr()?;
    parser.expect_end()?;
    Ok(result)
}
