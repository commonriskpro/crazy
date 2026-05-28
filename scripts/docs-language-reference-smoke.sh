#!/usr/bin/env bash
# Static smoke checks for the validation-stage language reference.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$ROOT_DIR/docs/language-reference.md"
PARSER="$ROOT_DIR/crates/ail-change/src/parser.rs"
PARSER_TESTS="$ROOT_DIR/crates/ail-change/src/parser_tests.rs"
OP_SCHEMA="$ROOT_DIR/crates/ail-change/src/op_schema.rs"
CANONICAL_OPS="$ROOT_DIR/crates/ail-change/src/canonical_ops.rs"
EXPR_TESTS="$ROOT_DIR/crates/ail-compiler/src/expr_parser_tests.rs"
CLI_TEST="$ROOT_DIR/crates/ail-cli/tests/cli_subcommands.rs"

require_literal() {
  local file="$1"
  local literal="$2"
  local label="$3"

  if ! grep -qF "$literal" "$file"; then
    printf 'missing %s in %s: %s\n' "$label" "$file" "$literal" >&2
    return 1
  fi
}

require_literal "$DOC" "<!-- Status: Implemented subset." "implemented-subset status"
require_literal "$DOC" "not a production-readiness claim" "production caveat"
require_literal "$DOC" "Semantic Graph" "semantic graph source-of-truth framing"
require_literal "$DOC" "change -> verify -> apply" "verification-first flow"
require_literal "$DOC" "A verb can parse and still become a no-op" "parser/canonicalizer caveat"
require_literal "$DOC" "op create_function id=fn.hello return=Text body=let(s, \"Hello, world!\", s)" "text hello example"
require_literal "$DOC" "op create_capability id=log.write" "capability declaration example"
require_literal "$DOC" "op grant target=fn.print_hello capability=log.write" "capability grant example"
require_literal "$DOC" "body=print(\"Hello, world!\")" "print body example"
require_literal "$DOC" "native linked execution" "native limitation"

for verb in create set add remove delete disconnect rename move replace connect bind expose hide grant revoke infer derive generate assert lock refactor migrate approve reject deprecate annotate verify; do
  require_literal "$DOC" "\`$verb" "verb family $verb"
  require_literal "$PARSER" "matches(verb, \"$verb\")" "parser verb family $verb"
done

for op in create_function create_type create_module create_capability add_param set_return add_effect remove_effect add_contract connect disconnect expose hide rename grant revoke deprecate annotate bind_handler infer_boundary; do
  require_literal "$DOC" "\`$op" "schema op $op"
  require_literal "$OP_SCHEMA" "verb_prefix: \"$op\"" "schema source $op"
done

require_literal "$PARSER_TESTS" "fn parse_all_new_verb_variants()" "new verb parser test"
require_literal "$PARSER_TESTS" "fn parse_kv_args_keeps_parenthesized_body_with_spaces()" "body spacing parser test"
require_literal "$CANONICAL_OPS" "OpPayload::Noop" "canonical no-op boundary"
require_literal "$CLI_TEST" "op create_function id=fn.hello return=Text body=let(s, \"Hello, world!\", s)" "CLI text fixture"
require_literal "$CLI_TEST" "op create_function id=fn.print_hello return=Int body=print(\"Hello, world!\")" "CLI print fixture"
require_literal "$CLI_TEST" "capability denied: log.write" "CLI capability denial assertion"

require_literal "$DOC" "add(x, y)" "add expression reference"
require_literal "$DOC" "let(total, add(x, y), if(gt(total, 10), total, 0))" "let expression reference"
require_literal "$DOC" "match(result, Ok(val), val, Err(e), -1)" "match expression reference"
require_literal "$DOC" "print(\"Hello, world!\")" "print expression reference"
require_literal "$DOC" "effect_call(database.read, Cart, cartId)" "effect expression reference"
require_literal "$DOC" "lambda(x, add(x, 1))" "lambda expression reference"
require_literal "$DOC" "foreach(item, items, add(acc, item))" "foreach expression reference"
require_literal "$DOC" "fold(0, items, add_item)" "fold expression reference"
require_literal "$DOC" "cell_new(0)" "cell expression reference"
require_literal "$DOC" "map(x, add(x, 1))" "map expression reference"
require_literal "$DOC" "set(add(x, 1), mul(y, 2))" "set expression reference"
require_literal "$DOC" "index(lst, add(i, 1))" "index expression reference"
require_literal "$DOC" "abort(\"unreachable branch\")" "abort expression reference"

require_literal "$EXPR_TESTS" "parse_expr(\"add(x, y)\")" "add expr parser evidence"
require_literal "$EXPR_TESTS" "parse_expr(\"let(total, add(x, y), if(gt(total, 10), total, 0))\")" "let expr parser evidence"
require_literal "$EXPR_TESTS" "parse_expr(\"match(result, Ok(val), val, Err(e), -1)\")" "match expr parser evidence"
require_literal "$EXPR_TESTS" 'parse_expr("print(\"Hello, world!\")")' "print expr parser evidence"
require_literal "$EXPR_TESTS" "parse_expr(\"effect_call(database.read, Cart, cartId)\")" "effect call parser evidence"
require_literal "$EXPR_TESTS" "parse_expr(\"lambda(x, add(x, 1))\")" "lambda parser evidence"
require_literal "$EXPR_TESTS" "parse_expr(\"foreach(item, items, add(acc, item))\")" "foreach parser evidence"
require_literal "$EXPR_TESTS" "parse_expr(\"fold(0, items, add_item)\")" "fold parser evidence"
require_literal "$EXPR_TESTS" "parse_expr(\"cell_new(0)\")" "cell parser evidence"
require_literal "$EXPR_TESTS" "parse_expr(\"map(x, add(x, 1))\")" "map parser evidence"
require_literal "$EXPR_TESTS" "parse_expr(\"set(add(x, 1), mul(y, 2))\")" "set parser evidence"
require_literal "$EXPR_TESTS" "parse_expr(\"index(lst, add(i, 1))\")" "index parser evidence"
require_literal "$EXPR_TESTS" 'parse_expr("abort(\"unreachable branch\")")' "abort parser evidence"

printf 'docs language reference smoke passed\n'
