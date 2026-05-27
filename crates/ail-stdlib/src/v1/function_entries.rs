use ail_core::semantic_graph::{CapabilityReqs, ContractClauses, EffectRow, NodeKind, TypeFacts};

use crate::exec::{FunctionImpl, stdlib_function_entries};
use crate::registry::{StabilityTier, StdlibEntry, StdlibId, StdlibRegistry};

use super::module_entries::v1_registry;

/// Return the extended v1 stdlib registry with `NodeKind::Function` entries.
///
/// Starts from `v1_registry()` (the 9-module base) and appends one
/// `StdlibEntry` per implemented function in:
///
/// - `std.numeric`: `checked_add`, `wrapping_add`, `saturating_add`,
///   `checked_sub`, `checked_mul`
/// - `std.core.option`: `map`, `and_then`, `unwrap_or`, `transpose`,
///   `collect_results`
/// - `std.core.result`: `map`, `and_then`, `unwrap_or`, `transpose`
/// - `std.text`: `trim`, `split`, `join`, `length_graphemes`,
///   `to_bytes`, `from_bytes`, `starts_with`, `ends_with`, `contains`, `replace`
/// - `std.iter`: `map`, `filter`, `fold`, `traverse`
///
/// The returned registry is guaranteed to pass `validate()`.
pub fn v1_registry_with_functions() -> StdlibRegistry {
    let mut reg = v1_registry();

    // ── std.numeric functions ─────────────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.checked_add".to_string()),
        module_path: "std::numeric".to_string(),
        name: "checked_add".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["i64".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["no silent overflow".to_string()],
            ensures: vec!["returns None on overflow".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.wrapping_add".to_string()),
        module_path: "std::numeric".to_string(),
        name: "wrapping_add".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "i64".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["wrapping semantics chosen explicitly".to_string()],
            ensures: vec!["result wraps on overflow (defined, not silent)".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.saturating_add".to_string()),
        module_path: "std::numeric".to_string(),
        name: "saturating_add".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "i64".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["saturating semantics chosen explicitly".to_string()],
            ensures: vec!["result clamped to i64::MAX or i64::MIN on overflow".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.checked_sub".to_string()),
        module_path: "std::numeric".to_string(),
        name: "checked_sub".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["i64".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["no silent underflow".to_string()],
            ensures: vec!["returns None on underflow or overflow".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.checked_mul".to_string()),
        module_path: "std::numeric".to_string(),
        name: "checked_mul".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["i64".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["no silent overflow".to_string()],
            ensures: vec!["returns None on overflow".to_string()],
        }),
    });

    // ── std.core.option functions ─────────────────────────────────────────
    //
    // IDs use the std.core.* namespace to match exec handler registration in
    // exec/registry.rs.  The dedup loop (below) skips entries already present,
    // so these pre-loop entries are what carry the contract_clauses.

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.map".to_string()),
        module_path: "std::core".to_string(),
        name: "map".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Option<T>".to_string()],
            ensures: vec![
                "None returns None without calling f".to_string(),
                "Some(v) returns Some(f(v))".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.and_then".to_string()),
        module_path: "std::core".to_string(),
        name: "and_then".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Option<T>".to_string()],
            ensures: vec![
                "None short-circuits without calling f".to_string(),
                "Some(v) returns f(v)".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.unwrap_or".to_string()),
        module_path: "std::core".to_string(),
        name: "unwrap_or".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "T".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Option<T>".to_string()],
            ensures: vec![
                "None returns the default value".to_string(),
                "Some(v) returns v".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.transpose".to_string()),
        module_path: "std::core".to_string(),
        name: "transpose".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["Option".to_string(), "T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Option<Result<T, E>>".to_string()],
            ensures: vec![
                "Some(Ok(v)) -> Ok(Some(v))".to_string(),
                "Some(Err(e)) -> Err(e)".to_string(),
                "None -> Ok(None)".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.collect_results".to_string()),
        module_path: "std::core".to_string(),
        name: "collect_results".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["List".to_string(), "T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is List<Result<T, E>>".to_string()],
            ensures: vec![
                "Ok(List<T>) when all items are Ok".to_string(),
                "Err(e) on the first Err encountered".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.ok_or".to_string()),
        module_path: "std::core".to_string(),
        name: "ok_or".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Option<T>".to_string(),
                "second arg is the error value E".to_string(),
            ],
            ensures: vec![
                "Some(v) returns Ok(v)".to_string(),
                "None returns Err(err)".to_string(),
            ],
        }),
    });

    // ── std.core.result functions ─────────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.result.map".to_string()),
        module_path: "std::core".to_string(),
        name: "map".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["T".to_string(), "U".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Result<T, E>".to_string()],
            ensures: vec![
                "Err(e) passes through unchanged without calling f".to_string(),
                "Ok(v) returns Ok(f(v))".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.result.and_then".to_string()),
        module_path: "std::core".to_string(),
        name: "and_then".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["T".to_string(), "U".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Result<T, E>".to_string()],
            ensures: vec![
                "Err(e) short-circuits without calling f".to_string(),
                "Ok(v) returns f(v)".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.result.unwrap_or".to_string()),
        module_path: "std::core".to_string(),
        name: "unwrap_or".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "T".to_string(),
            generics: vec!["T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Result<T, E>".to_string()],
            ensures: vec![
                "Err returns the default value".to_string(),
                "Ok(v) returns v".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.result.transpose".to_string()),
        module_path: "std::core".to_string(),
        name: "transpose".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["Result".to_string(), "T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Result<Option<T>, E>".to_string()],
            ensures: vec![
                "Ok(Some(v)) -> Some(Ok(v))".to_string(),
                "Ok(None) -> None".to_string(),
                "Err(e) -> Some(Err(e))".to_string(),
            ],
        }),
    });

    // ── std.text functions ────────────────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.trim".to_string()),
        module_path: "std::text".to_string(),
        name: "trim".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Text".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["valid UTF-8 input".to_string()],
            ensures: vec!["result is valid Text with no leading/trailing whitespace".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.split".to_string()),
        module_path: "std::text".to_string(),
        name: "split".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["Text".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["valid UTF-8 input".to_string()],
            ensures: vec!["result is valid List<Text>".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.join".to_string()),
        module_path: "std::text".to_string(),
        name: "join".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Text".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["valid UTF-8 inputs".to_string()],
            ensures: vec!["result is valid Text".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.length_graphemes".to_string()),
        module_path: "std::text".to_string(),
        name: "length_graphemes".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "UInt".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["valid UTF-8 input".to_string()],
            ensures: vec!["result >= 0".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.to_bytes".to_string()),
        module_path: "std::text".to_string(),
        name: "to_bytes".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bytes".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.from_bytes".to_string()),
        module_path: "std::text".to_string(),
        name: "from_bytes".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["Text".to_string(), "DecodeError".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![],
            ensures: vec![
                "Ok(Text) if bytes are valid UTF-8".to_string(),
                "Err(DecodeError) if bytes are invalid UTF-8".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.regex".to_string()),
        module_path: "std::text".to_string(),
        name: "regex".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["pattern is a valid regex".to_string()],
            ensures: vec!["returns Bool indicating whether pattern matches the input".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.starts_with".to_string()),
        module_path: "std::text".to_string(),
        name: "starts_with".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both arguments are valid UTF-8".to_string()],
            ensures: vec![
                "returns true if and only if the first argument begins with the prefix".to_string(),
                "empty prefix always returns true".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.ends_with".to_string()),
        module_path: "std::text".to_string(),
        name: "ends_with".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both arguments are valid UTF-8".to_string()],
            ensures: vec![
                "returns true if and only if the first argument ends with the suffix".to_string(),
                "empty suffix always returns true".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.contains".to_string()),
        module_path: "std::text".to_string(),
        name: "contains".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both arguments are valid UTF-8".to_string()],
            ensures: vec![
                "returns true if and only if needle appears as a substring".to_string(),
                "empty needle always returns true".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.replace".to_string()),
        module_path: "std::text".to_string(),
        name: "replace".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Text".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["all arguments are valid UTF-8".to_string()],
            ensures: vec![
                "every non-overlapping occurrence of `from` is replaced with `to`".to_string(),
                "empty `from` returns the input unchanged".to_string(),
            ],
        }),
    });

    // ── std.iter functions ────────────────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.iter.map".to_string()),
        module_path: "std::iter".to_string(),
        name: "map".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: Some(EffectRow {
            effects: vec!["EffectPoly".to_string()],
        }),
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "input is List<T>".to_string(),
                "f is a total function T -> U".to_string(),
            ],
            ensures: vec![
                "output length equals input length".to_string(),
                "output[i] = f(input[i]) for every i".to_string(),
                "empty input returns empty list".to_string(),
                "effects of f are preserved (EffectPoly)".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.iter.filter".to_string()),
        module_path: "std::iter".to_string(),
        name: "filter".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "input is List<T>".to_string(),
                "pred is a total predicate T -> Bool".to_string(),
            ],
            ensures: vec![
                "result is a subsequence of input".to_string(),
                "every retained element satisfies pred".to_string(),
                "relative order of retained elements is preserved".to_string(),
                "empty input returns empty list".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.iter.fold".to_string()),
        module_path: "std::iter".to_string(),
        name: "fold".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "U".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: Some(EffectRow {
            effects: vec!["EffectPoly".to_string()],
        }),
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "input is List<T>".to_string(),
                "init is U (accumulator seed)".to_string(),
                "f receives one List([acc, item]) binary-encoded pair (acc: U, item: T) and returns U".to_string(),
            ],
            ensures: vec![
                "empty input returns init unchanged".to_string(),
                "result is left fold of f over items starting from init".to_string(),
                "effects of f are preserved (EffectPoly)".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.iter.traverse".to_string()),
        module_path: "std::iter".to_string(),
        name: "traverse".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec![
                "List".to_string(),
                "T".to_string(),
                "U".to_string(),
                "E".to_string(),
            ],
        }),
        effect_row: Some(EffectRow {
            effects: vec!["EffectPoly".to_string()],
        }),
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "input is List<T>".to_string(),
                "f is a total function T -> Result<U, E>".to_string(),
            ],
            ensures: vec![
                "Ok(List<U>) when all applications of f succeed".to_string(),
                "Err(e) from the first failed application of f".to_string(),
                "short-circuits: no elements after the first Err are evaluated".to_string(),
                "effects of f are preserved (EffectPoly)".to_string(),
            ],
        }),
    });

    // ── std.numeric narrowing functions ───────────────────────────────────
    //
    // Pre-loop entries so contract_clauses survive the dedup loop, which
    // always injects with contract_clauses: None for new entries.

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.narrow_to_i32".to_string()),
        module_path: "std::numeric".to_string(),
        name: "narrow_to_i32".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["Int32".to_string(), "ArithError".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Int (i64)".to_string()],
            ensures: vec![
                "Ok(v) when value fits in i32 range".to_string(),
                "Err on overflow or underflow".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.narrow_to_u32".to_string()),
        module_path: "std::numeric".to_string(),
        name: "narrow_to_u32".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["UInt32".to_string(), "ArithError".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Int (i64)".to_string()],
            ensures: vec![
                "Ok(v) when value fits in u32 range (0..=4294967295)".to_string(),
                "Err on negative values or overflow".to_string(),
            ],
        }),
    });

    // ── std.bytes functions ───────────────────────────────────────────────
    //
    // Pre-loop entries so contract_clauses survive the dedup loop.

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.bytes.length".to_string()),
        module_path: "std::bytes".to_string(),
        name: "length".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Int".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Bytes".to_string()],
            ensures: vec![
                "result >= 0".to_string(),
                "result equals the number of bytes in the buffer".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.bytes.at".to_string()),
        module_path: "std::bytes".to_string(),
        name: "at".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["Int".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Bytes".to_string(),
                "second arg is Int (index)".to_string(),
            ],
            ensures: vec![
                "Some(v) where v is in 0..=255 when 0 <= index < length".to_string(),
                "None when index is negative or >= length".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.bytes.slice".to_string()),
        module_path: "std::bytes".to_string(),
        name: "slice".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["Bytes".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Bytes".to_string(),
                "second and third args are Int (start, end)".to_string(),
            ],
            ensures: vec![
                "Some(Bytes) containing [start..end] bytes when 0 <= start <= end <= length"
                    .to_string(),
                "None when start or end is negative, start > end, or end > length".to_string(),
                "Some(empty Bytes) when start == end and both are in bounds".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.bytes.concat".to_string()),
        module_path: "std::bytes".to_string(),
        name: "concat".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bytes".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both args are Bytes".to_string()],
            ensures: vec![
                "result contains all bytes of the first buffer followed by all bytes of the second"
                    .to_string(),
                "neither input is mutated".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.bytes.empty".to_string()),
        module_path: "std::bytes".to_string(),
        name: "empty".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Bytes".to_string()],
            ensures: vec![
                "true when buffer has zero bytes".to_string(),
                "false when buffer has one or more bytes".to_string(),
            ],
        }),
    });

    // ── std.collections list functions ───────────────────────────────────
    //
    // Pre-loop entries so contract_clauses survive the dedup loop, which
    // always injects contract_clauses: None for new entries.

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.length".to_string()),
        module_path: "std::collections".to_string(),
        name: "length".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "UInt".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["first arg is List<T>".to_string()],
            ensures: vec![
                "result >= 0".to_string(),
                "result equals the number of elements in the list".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.push".to_string()),
        module_path: "std::collections".to_string(),
        name: "push".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is List<T>".to_string(),
                "second arg is T".to_string(),
            ],
            ensures: vec![
                "result length equals input length plus one".to_string(),
                "new element is appended at the end".to_string(),
                "original list is not mutated".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.get".to_string()),
        module_path: "std::collections".to_string(),
        name: "get".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is List<T>".to_string(),
                "second arg is Int (index)".to_string(),
            ],
            ensures: vec![
                "Some(element) when 0 <= index < length".to_string(),
                "None when index >= length".to_string(),
                "None when index < 0".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.map".to_string()),
        module_path: "std::collections".to_string(),
        name: "map".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is List<T>".to_string(),
                "second arg is Fn(T) -> U".to_string(),
            ],
            ensures: vec![
                "result length equals input length".to_string(),
                "each result element is f applied to the corresponding input element".to_string(),
                "order is preserved".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.filter".to_string()),
        module_path: "std::collections".to_string(),
        name: "filter".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is List<T>".to_string(),
                "second arg is Fn(T) -> Bool".to_string(),
            ],
            ensures: vec![
                "result contains only elements where predicate returns true".to_string(),
                "relative order of retained elements is preserved".to_string(),
                "result length <= input length".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.fold".to_string()),
        module_path: "std::collections".to_string(),
        name: "fold".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "U".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is List<T>".to_string(),
                "second arg is initial accumulator U".to_string(),
                "third arg is Fn(List([acc, item])) -> U (binary encoding: function receives List([acc, item]))".to_string(),
            ],
            ensures: vec![
                "empty list returns the initial accumulator unchanged".to_string(),
                "fold function is applied left-to-right".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.concat".to_string()),
        module_path: "std::collections".to_string(),
        name: "concat".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both args are List<T>".to_string()],
            ensures: vec![
                "result contains all elements of the first list followed by the second".to_string(),
                "result length equals sum of both input lengths".to_string(),
                "neither input list is mutated".to_string(),
            ],
        }),
    });

    // ── std.collections map functions ─────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.map.get".to_string()),
        module_path: "std::collections".to_string(),
        name: "get".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["V".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Map<Text, V>".to_string(),
                "second arg is Text (key)".to_string(),
            ],
            ensures: vec![
                "Some(value) when key exists in the map".to_string(),
                "None when key is absent".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.map.insert".to_string()),
        module_path: "std::collections".to_string(),
        name: "insert".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Map".to_string(),
            generics: vec!["Text".to_string(), "V".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Map<Text, V>".to_string(),
                "second arg is Text (key)".to_string(),
                "third arg is V (value)".to_string(),
            ],
            ensures: vec![
                "result contains the new key-value pair".to_string(),
                "any existing entry at key is replaced".to_string(),
                "original map is not mutated".to_string(),
            ],
        }),
    });

    // ── std.collections set functions ─────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.set.contains".to_string()),
        module_path: "std::collections".to_string(),
        name: "contains".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is List<T> (set representation)".to_string(),
                "second arg is T (element to test)".to_string(),
            ],
            ensures: vec![
                "true when element is equal to at least one entry".to_string(),
                "false when no entry matches".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.set.insert".to_string()),
        module_path: "std::collections".to_string(),
        name: "insert".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is List<T> (set representation)".to_string(),
                "second arg is T (element to insert)".to_string(),
            ],
            ensures: vec![
                "result contains the element".to_string(),
                "no duplicate entries are introduced".to_string(),
                "original set is not mutated".to_string(),
            ],
        }),
    });

    // ── std.time pure functions ───────────────────────────────────────────
    //
    // Pre-loop entries so contract_clauses survive the dedup loop.

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.time.duration_since".to_string()),
        module_path: "std::time".to_string(),
        name: "duration_since".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Int".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both args are Int (millisecond epoch instants)".to_string()],
            ensures: vec![
                "result is (first - second) in milliseconds".to_string(),
                "result is negative when second instant is later than first".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.time.add_duration".to_string()),
        module_path: "std::time".to_string(),
        name: "add_duration".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Int".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Int (millisecond epoch instant)".to_string(),
                "second arg is Int (duration in milliseconds)".to_string(),
            ],
            ensures: vec!["result is the sum of instant and duration in milliseconds".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.time.instant_to_ms".to_string()),
        module_path: "std::time".to_string(),
        name: "instant_to_ms".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Int".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Int (epoch-millisecond instant)".to_string()],
            ensures: vec![
                "result is the same Int value (identity projection for epoch-ms instants)"
                    .to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.time.now".to_string()),
        module_path: "std::time".to_string(),
        name: "now".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Instant".to_string(),
            generics: vec![],
        }),
        effect_row: Some(EffectRow {
            effects: vec!["clock.now".to_string()],
        }),
        capability_reqs: Some(CapabilityReqs {
            caps: vec!["clock.now".to_string()],
        }),
        contract_clauses: Some(ContractClauses {
            requires: vec!["clock.now capability must be granted".to_string()],
            ensures: vec![
                "result is Instant (runtime representation: Int epoch-ms since Unix epoch)"
                    .to_string(),
                "result > 0 for any real-world wall-clock call".to_string(),
            ],
        }),
    });

    for function in stdlib_function_entries() {
        if reg.entries.iter().any(|entry| entry.id.0 == function.id) {
            continue;
        }

        let (effect_row, capability_reqs) = match function.implementation {
            FunctionImpl::Pure(_) => (None, None),
            FunctionImpl::Capability { capability, .. } => {
                let caps = vec![capability.to_string()];
                (
                    Some(EffectRow {
                        effects: caps.clone(),
                    }),
                    Some(CapabilityReqs { caps }),
                )
            }
        };

        reg.entries.push(StdlibEntry {
            id: StdlibId(function.id.to_string()),
            module_path: function.module.replace('.', "::"),
            name: function.name.to_string(),
            kind: NodeKind::Function,
            stability: StabilityTier::Stable,
            type_facts: Some(TypeFacts {
                nominal: function.return_type.to_string(),
                generics: function
                    .params
                    .iter()
                    .map(|param| (*param).to_string())
                    .collect(),
            }),
            effect_row,
            capability_reqs,
            contract_clauses: None,
        });
    }

    reg
}
