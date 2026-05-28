// ── ail-change::op_schema ─────────────────────────────────────────────────
//
// Op-schema validation layer (doc §Op schema model).
//
// # Design
//
// Each operation has a schema that declares which key/value arguments are
// required and which are optional.  The validator checks every `ParsedOp`
// against the registered schema for its verb and returns a list of errors.
//
// The grammar layer (`parser.rs`) validates *syntax* (verb recognised,
// key=value well-formed).  The op-schema layer validates *semantics* —
// required arguments are present — before the canonicalizer runs.
//
// # Usage
//
// ```
// let errors = validate_op_schemas(&parsed_changeset);
// if !errors.is_empty() { /* reject the change */ }
// ```
//
// # Extensibility
//
// `OP_SCHEMAS` is a static slice.  Adding a new schema requires only a new
// `OpSchemaEntry` in that slice; no other code changes are needed.

use crate::parser::{ParsedChangeSet, ParsedOp};

// ── Error type ────────────────────────────────────────────────────────────

/// An error produced by op-schema validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpSchemaError {
    /// A required argument is missing from an op.
    MissingRequiredArg {
        /// The verb of the failing op (e.g. `"add_param"`).
        verb: String,
        /// The missing argument key (e.g. `"type"`).
        arg: String,
    },
}

impl std::fmt::Display for OpSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpSchemaError::MissingRequiredArg { verb, arg } => {
                write!(f, "op '{verb}' requires argument '{arg}'")
            }
        }
    }
}

impl std::error::Error for OpSchemaError {}

// ── Schema registry ───────────────────────────────────────────────────────

/// A single op-schema entry: verb prefix and its required arguments.
///
/// Matching is prefix-based: an entry with `verb_prefix = "add_param"` matches
/// only that exact verb.  An entry with `verb_prefix = "create_function"`
/// matches only `create_function`.
struct OpSchemaEntry {
    /// Full verb or verb prefix to match (exact match used here).
    verb_prefix: &'static str,
    /// Argument keys that MUST be present on every op with this verb.
    required: &'static [&'static str],
}

/// Static registry of op schemas (doc §Op schema model examples).
///
/// Only the most constrained ops are listed; others are implicitly unconstrained
/// (any args are accepted).  The list grows as new schemas are formalised.
static OP_SCHEMAS: &[OpSchemaEntry] = &[
    // create_function: requires id
    OpSchemaEntry {
        verb_prefix: "create_function",
        required: &["id"],
    },
    // create_test: requires id; body is optional so pending tests can be declared.
    OpSchemaEntry {
        verb_prefix: "create_test",
        required: &["id"],
    },
    // create_type: requires id
    OpSchemaEntry {
        verb_prefix: "create_type",
        required: &["id"],
    },
    // create_module: requires id
    OpSchemaEntry {
        verb_prefix: "create_module",
        required: &["id"],
    },
    // create_capability: requires id
    OpSchemaEntry {
        verb_prefix: "create_capability",
        required: &["id"],
    },
    // add_param: requires target, name, type
    OpSchemaEntry {
        verb_prefix: "add_param",
        required: &["target", "name", "type"],
    },
    // set_return: requires target, type
    OpSchemaEntry {
        verb_prefix: "set_return",
        required: &["target", "type"],
    },
    // add_effect: requires target, effect
    OpSchemaEntry {
        verb_prefix: "add_effect",
        required: &["target", "effect"],
    },
    // remove_effect: requires target, effect
    OpSchemaEntry {
        verb_prefix: "remove_effect",
        required: &["target", "effect"],
    },
    // add_contract: requires target, kind, rule
    OpSchemaEntry {
        verb_prefix: "add_contract",
        required: &["target", "kind", "rule"],
    },
    // connect: requires source, target (relation is defaulted to DependsOn)
    OpSchemaEntry {
        verb_prefix: "connect",
        required: &["source", "target"],
    },
    // disconnect: requires source, target
    OpSchemaEntry {
        verb_prefix: "disconnect",
        required: &["source", "target"],
    },
    // expose: requires target
    OpSchemaEntry {
        verb_prefix: "expose",
        required: &["target"],
    },
    // hide: requires target
    OpSchemaEntry {
        verb_prefix: "hide",
        required: &["target"],
    },
    // rename: requires target, name
    OpSchemaEntry {
        verb_prefix: "rename",
        required: &["target", "name"],
    },
    // grant: requires target, capability
    OpSchemaEntry {
        verb_prefix: "grant",
        required: &["target", "capability"],
    },
    // revoke: requires target, capability
    OpSchemaEntry {
        verb_prefix: "revoke",
        required: &["target", "capability"],
    },
    // deprecate: requires target
    OpSchemaEntry {
        verb_prefix: "deprecate",
        required: &["target"],
    },
    // annotate: requires target, key, value
    OpSchemaEntry {
        verb_prefix: "annotate",
        required: &["target", "key", "value"],
    },
    // bind_handler: requires capability, handler
    OpSchemaEntry {
        verb_prefix: "bind_handler",
        required: &["capability", "handler"],
    },
    // infer_boundary: requires target
    OpSchemaEntry {
        verb_prefix: "infer_boundary",
        required: &["target"],
    },
];

// ── validate_op_schemas ───────────────────────────────────────────────────

/// Validate all parsed ops against their registered op schemas.
///
/// Returns a (possibly empty) list of errors.  An empty list means every
/// op satisfies its schema.
///
/// Ops whose verb is not listed in `OP_SCHEMAS` pass validation without error
/// (they are treated as unconstrained/unknown ops).
pub fn validate_op_schemas(pcs: &ParsedChangeSet) -> Vec<OpSchemaError> {
    pcs.parsed_ops.iter().flat_map(check_op).collect()
}

fn check_op(op: &ParsedOp) -> Vec<OpSchemaError> {
    // Find the schema entry for this verb (exact match).
    let Some(schema) = OP_SCHEMAS.iter().find(|s| s.verb_prefix == op.verb) else {
        // No schema registered — unconstrained; pass.
        return vec![];
    };

    schema
        .required
        .iter()
        .filter(|&&arg| !op.args.contains_key(arg))
        .map(|&arg| OpSchemaError::MissingRequiredArg {
            verb: op.verb.clone(),
            arg: arg.to_string(),
        })
        .collect()
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_changeset;

    fn changeset_with_ops(ops_body: &str) -> ParsedChangeSet {
        let src = format!("change test\nauthor tester\nbase 0\n{ops_body}\nend\n");
        parse_changeset(&src).expect("fixture must parse")
    }

    // Scenario: valid op with all required args produces no errors.
    //   GIVEN `op create_function id=fn.x` (id is required)
    //   WHEN validate_op_schemas is called
    //   THEN no errors returned
    #[test]
    fn valid_create_function_produces_no_errors() {
        let pcs = changeset_with_ops("op create_function id=fn.x");
        let errors = validate_op_schemas(&pcs);
        assert!(
            errors.is_empty(),
            "valid op must have no errors: {errors:?}"
        );
    }

    // Scenario: missing required arg produces MissingRequiredArg error.
    //   GIVEN `op add_param target=fn.x name=cart` (type is missing)
    //   WHEN validate_op_schemas is called
    //   THEN one error: MissingRequiredArg { verb: "add_param", arg: "type" }
    #[test]
    fn missing_required_arg_returns_error() {
        let pcs = changeset_with_ops("op add_param target=fn.x name=cart");
        let errors = validate_op_schemas(&pcs);
        assert_eq!(errors.len(), 1, "expected exactly one error: {errors:?}");
        assert!(
            matches!(&errors[0], OpSchemaError::MissingRequiredArg { verb, arg }
                if verb == "add_param" && arg == "type"),
            "error must identify missing 'type' arg: {:?}",
            errors[0]
        );
    }

    // Scenario: unknown verb is not rejected by op-schema layer.
    //   GIVEN an op verb not in OP_SCHEMAS
    //   WHEN validate_op_schemas is called
    //   THEN no errors (parser already accepted it)
    #[test]
    fn unknown_verb_not_in_schema_passes_validation() {
        // `set_body` has no schema entry — must pass unconstrained.
        let pcs = changeset_with_ops("op set_body target=fn.x body=foo");
        let errors = validate_op_schemas(&pcs);
        assert!(errors.is_empty(), "unconstrained op must have no errors");
    }

    // TRIANGULATE: multiple missing args all reported.
    #[test]
    fn multiple_missing_args_all_reported() {
        // `add_param` requires target, name, type — all missing.
        let pcs = changeset_with_ops("op add_param");
        let errors = validate_op_schemas(&pcs);
        let missing_keys: std::collections::BTreeSet<&str> = errors
            .iter()
            .filter_map(|e| match e {
                OpSchemaError::MissingRequiredArg { verb, arg } => {
                    (verb == "add_param").then_some(arg.as_str())
                }
            })
            .collect();
        assert!(missing_keys.contains("target"), "target must be reported");
        assert!(missing_keys.contains("name"), "name must be reported");
        assert!(missing_keys.contains("type"), "type must be reported");
    }

    // Scenario: changeset with multiple ops, only one invalid.
    #[test]
    fn one_invalid_op_in_multi_op_changeset() {
        let pcs = changeset_with_ops(
            "op create_function id=fn.checkout\nop add_param target=fn.checkout name=x",
        );
        let errors = validate_op_schemas(&pcs);
        assert_eq!(errors.len(), 1, "only the incomplete add_param must error");
    }
}
