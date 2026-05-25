// ── ail-stdlib::v1 ────────────────────────────────────────────────────────
//
// Canonical v1 stdlib module registry.
//
// # v1 module gate
//
// The v1 registry contains all modules listed in docs/stdlib.md.
//
// # Entry order
//
// Declaration order is stable and determines `NodeRef` assignment during
// graph projection.  Do not reorder entries after the v1 registry ships.
//
// # Metadata conventions
//
// `type_facts.nominal` names the primary exported type (not the module).
// `effect_row` is populated for effect-bearing or effect-polymorphic modules.
// `capability_reqs` is populated only for `std.capability` (the definition module).
// `contract_clauses` carries module-level invariants from docs/stdlib.md.
//
// # G26: Function entries
//
// `v1_registry_with_functions()` extends the base module registry with
// `NodeKind::Function` entries for each semantic function implementation
// in the std.numeric, std.option, std.result, std.text, and std.iter modules.
// The base `v1_registry()` is preserved unchanged for backward compatibility.

use ail_core::semantic_graph::{CapabilityReqs, ContractClauses, EffectRow, NodeKind, TypeFacts};

use crate::capability;
use crate::exec::{FunctionImpl, stdlib_function_entries};
use crate::registry::{StabilityTier, StdlibEntry, StdlibId, StdlibRegistry};

fn module_entry(
    id: &str,
    name: &str,
    nominal: &str,
    generics: &[&str],
    effect_row: Option<Vec<&str>>,
    capability_reqs: Option<Vec<&str>>,
    contract_clauses: Option<ContractClauses>,
) -> StdlibEntry {
    StdlibEntry {
        id: StdlibId(id.to_string()),
        module_path: format!("std::{}", name),
        name: name.to_string(),
        kind: NodeKind::Module,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: nominal.to_string(),
            generics: generics.iter().map(|g| (*g).to_string()).collect(),
        }),
        effect_row: effect_row.map(|effects| EffectRow {
            effects: effects.into_iter().map(str::to_string).collect(),
        }),
        capability_reqs: capability_reqs.map(|caps| CapabilityReqs {
            caps: caps.into_iter().map(str::to_string).collect(),
        }),
        contract_clauses,
    }
}

fn append_full_stdlib_modules(reg: &mut StdlibRegistry) {
    reg.entries.extend([
        module_entry(
            "std.decimal",
            "decimal",
            "Decimal",
            &["Scale", "Precision"],
            None,
            None,
            Some(ContractClauses {
                requires: vec!["rounding policy explicit when needed".to_string()],
                ensures: vec!["no silent narrowing".to_string()],
            }),
        ),
        module_entry(
            "std.encoding",
            "encoding",
            "Encoder",
            &["T", "Format"],
            None,
            None,
            None,
        ),
        module_entry(
            "std.json",
            "json",
            "Json",
            &[],
            None,
            None,
            Some(ContractClauses {
                requires: vec!["visible schema for derived encoding".to_string()],
                ensures: vec!["decoders return Result".to_string()],
            }),
        ),
        module_entry(
            "std.time",
            "time",
            "Instant",
            &[],
            Some(vec![capability::CLOCK_NOW]),
            Some(vec![capability::CLOCK_NOW]),
            Some(ContractClauses {
                requires: vec!["no implicit global timezone".to_string()],
                ensures: vec!["now() is effectful".to_string()],
            }),
        ),
        module_entry(
            "std.random",
            "random",
            "DeterministicRng",
            &[],
            Some(vec![capability::RANDOM_GENERATE]),
            Some(vec![capability::RANDOM_GENERATE]),
            Some(ContractClauses {
                requires: vec!["randomness is not pure".to_string()],
                ensures: vec!["deterministic and crypto randomness are separate".to_string()],
            }),
        ),
        module_entry(
            "std.crypto",
            "crypto",
            "Hash",
            &[],
            Some(vec!["crypto.random.bytes", "secret.read"]),
            Some(vec!["crypto.random.bytes", "secret.read"]),
            Some(ContractClauses {
                requires: vec!["secrets not exposed as plain Text by default".to_string()],
                ensures: vec!["constant-time comparisons explicit".to_string()],
            }),
        ),
        module_entry(
            "std.io",
            "io",
            "FileHandle",
            &[],
            Some(vec![
                capability::IO_STDIN,
                capability::IO_STDOUT,
                capability::IO_STDERR,
            ]),
            Some(vec![
                capability::IO_STDIN,
                capability::IO_STDOUT,
                capability::IO_STDERR,
            ]),
            None,
        ),
        module_entry(
            "std.fs",
            "fs",
            "Path",
            &[],
            Some(vec![capability::FS_READ, capability::FS_WRITE]),
            Some(vec![capability::FS_READ, capability::FS_WRITE]),
            Some(ContractClauses {
                requires: vec!["file access requires grants".to_string()],
                ensures: vec!["paths are capability-constrained".to_string()],
            }),
        ),
        module_entry(
            "std.net",
            "net",
            "Url",
            &[],
            Some(vec![capability::NET_CONNECT]),
            Some(vec![capability::NET_CONNECT]),
            Some(ContractClauses {
                requires: vec!["network access requires grants".to_string()],
                ensures: vec!["hosts can be constrained".to_string()],
            }),
        ),
        module_entry(
            "std.http",
            "http",
            "HttpRequest",
            &[],
            Some(vec!["http.call"]),
            Some(vec!["http.call"]),
            Some(ContractClauses {
                requires: vec!["timeouts explicit".to_string()],
                ensures: vec!["retries explicit".to_string()],
            }),
        ),
        module_entry(
            "std.process",
            "process",
            "ProcessHandle",
            &[],
            Some(vec![capability::PROCESS_EXEC]),
            Some(vec![capability::PROCESS_EXEC]),
            Some(ContractClauses {
                requires: vec!["process access requires strict grants".to_string()],
                ensures: vec!["process operations are effectful".to_string()],
            }),
        ),
        module_entry(
            "std.env",
            "env",
            "EnvVar",
            &[],
            Some(vec![capability::ENV_READ, capability::ENV_WRITE]),
            Some(vec![capability::ENV_READ, capability::ENV_WRITE]),
            Some(ContractClauses {
                requires: vec!["env access requires strict grants".to_string()],
                ensures: vec!["env operations are effectful".to_string()],
            }),
        ),
        module_entry(
            "std.concurrent",
            "concurrent",
            "Task",
            &["T"],
            Some(vec!["clock.monotonic"]),
            None,
            Some(ContractClauses {
                requires: vec!["structured concurrency by default".to_string()],
                ensures: vec!["no orphan tasks".to_string()],
            }),
        ),
        module_entry(
            "std.sync",
            "sync",
            "Mutex",
            &["T"],
            None,
            None,
            Some(ContractClauses {
                requires: vec!["shared state requires safe type".to_string()],
                ensures: vec!["safe synchronization primitive".to_string()],
            }),
        ),
        module_entry(
            "std.log",
            "log",
            "LogLevel",
            &[],
            Some(vec![capability::LOG_EMIT]),
            Some(vec![capability::LOG_EMIT]),
            Some(ContractClauses {
                requires: vec!["logs are effects".to_string()],
                ensures: vec!["PII/secrets redacted by policy".to_string()],
            }),
        ),
        module_entry(
            "std.trace",
            "trace",
            "TraceId",
            &[],
            Some(vec![capability::TRACE_SPAN, "metric.emit"]),
            Some(vec![capability::TRACE_SPAN, "metric.emit"]),
            Some(ContractClauses {
                requires: vec!["traces are effects".to_string()],
                ensures: vec!["runtime audit separate from app logs".to_string()],
            }),
        ),
        module_entry(
            "std.testing",
            "testing",
            "Test",
            &[],
            None,
            None,
            Some(ContractClauses {
                requires: vec!["tests are evidence, not automatic proof".to_string()],
                ensures: vec!["property tests link to contracts/invariants".to_string()],
            }),
        ),
        module_entry(
            "std.boundary",
            "boundary",
            "BoundaryDef",
            &[],
            None,
            None,
            None,
        ),
        module_entry(
            "std.diagnostics",
            "diagnostics",
            "Diagnostic",
            &[],
            None,
            None,
            None,
        ),
        module_entry(
            "std.verify",
            "verify",
            "VerificationReport",
            &[],
            None,
            None,
            None,
        ),
        module_entry(
            "std.runtime",
            "runtime",
            "RuntimeProfile",
            &[],
            None,
            None,
            None,
        ),
    ]);
}

/// Return the canonical v1 stdlib registry.
///
/// Contains ordered entries corresponding to the v1 stdlib scope from
/// `docs/stdlib.md`.  All entries carry `StabilityTier::Stable`.  Semantic-fact fields
/// are populated to reflect the type, effect, capability, and contract
/// metadata documented in `docs/stdlib.md`.
///
/// The returned `StdlibRegistry` is guaranteed to pass `validate()`.
pub fn v1_registry() -> StdlibRegistry {
    let mut reg = StdlibRegistry {
        entries: vec![
            // 0 — std.core
            //
            // Base language helpers: Unit, Never, Bool, Ordering, Identity.
            // Interfaces: Eq, Hashable, Ord, PartialOrd, Debug, Display.
            // No effects, no capability requirements, no module-level contracts.
            StdlibEntry {
                id: StdlibId("std.core".to_string()),
                module_path: "std::core".to_string(),
                name: "core".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Module".to_string(),
                    generics: vec![],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 1 — std.option
            //
            // Option<T> = Some(T) | None.
            // Generic over one type parameter T.
            StdlibEntry {
                id: StdlibId("std.option".to_string()),
                module_path: "std::option".to_string(),
                name: "option".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Option".to_string(),
                    generics: vec!["T".to_string()],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 2 — std.result
            //
            // Result<T, E> = Ok(T) | Err(E).
            // Generic over two type parameters: value type T and error type E.
            StdlibEntry {
                id: StdlibId("std.result".to_string()),
                module_path: "std::result".to_string(),
                name: "result".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Result".to_string(),
                    generics: vec!["T".to_string(), "E".to_string()],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 3 — std.numeric
            //
            // Int, UInt, Int32, Int64, UInt32, UInt64, Float, Decimal<Scale, Precision>.
            // Generic over numeric kind N (the numeric type parameter).
            // Contracts: no silent overflow, no silent narrowing.
            StdlibEntry {
                id: StdlibId("std.numeric".to_string()),
                module_path: "std::numeric".to_string(),
                name: "numeric".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Numeric".to_string(),
                    generics: vec!["N".to_string()],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: Some(ContractClauses {
                    requires: vec![
                        "no silent overflow".to_string(),
                        "no silent narrowing".to_string(),
                    ],
                    ensures: vec!["result is defined numeric type".to_string()],
                }),
            },
            // 4 — std.text
            //
            // Text, CodePoint, Grapheme, NormalizedText<Form>.
            // Refinements: NonEmptyText, Email, Url, Slug.
            // Contract: input must be valid UTF-8; result is always valid Text.
            StdlibEntry {
                id: StdlibId("std.text".to_string()),
                module_path: "std::text".to_string(),
                name: "text".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Text".to_string(),
                    generics: vec![],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: Some(ContractClauses {
                    requires: vec!["valid UTF-8 input".to_string()],
                    ensures: vec!["result is valid Text".to_string()],
                }),
            },
            // 5 — std.bytes
            //
            // Bytes — raw byte sequence.  No generics, no effects, no contracts.
            StdlibEntry {
                id: StdlibId("std.bytes".to_string()),
                module_path: "std::bytes".to_string(),
                name: "bytes".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Bytes".to_string(),
                    generics: vec![],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 6 — std.collections
            //
            // List<T>, Set<T>, Map<K,V>, Vector<T,N>, OrderedSet<T>, OrderedMap<K,V>, Array<T>.
            // Generic over element type T.
            // Contracts: length >= 0; no duplicate keys in Map; ordering is explicit by type.
            StdlibEntry {
                id: StdlibId("std.collections".to_string()),
                module_path: "std::collections".to_string(),
                name: "collections".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Collection".to_string(),
                    generics: vec!["T".to_string()],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: Some(ContractClauses {
                    requires: vec!["length >= 0".to_string()],
                    ensures: vec![
                        "no duplicate keys in Map".to_string(),
                        "ordering is explicit by type".to_string(),
                    ],
                }),
            },
            // 7 — std.iter
            //
            // Effect-polymorphic iterator abstractions: map<T,U,e>, filter<T>,
            // fold<T,U,e>, traverse<T,U,E,e>.
            // Generic over element type T and effect parameter E.
            // EffectPoly: helpers preserve the effect parameter — they do not
            // hide or erase the caller's declared effects.
            StdlibEntry {
                id: StdlibId("std.iter".to_string()),
                module_path: "std::iter".to_string(),
                name: "iter".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Iterator".to_string(),
                    generics: vec!["T".to_string(), "E".to_string()],
                }),
                effect_row: Some(EffectRow {
                    effects: vec!["EffectPoly".to_string()],
                }),
                capability_reqs: None,
                contract_clauses: None,
            },
            // 8 — std.capability
            //
            // Common capability abstractions: CapabilityId, CapabilityGrant,
            // CapabilityManifest, HandlerBinding, HostResult<T>, HostError.
            // This module IS the canonical definition of all common capabilities.
            // It declares — but does not grant — each capability it exports.
            StdlibEntry {
                id: StdlibId("std.capability".to_string()),
                module_path: "std::capability".to_string(),
                name: "capability".to_string(),
                kind: NodeKind::Capability,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "CapabilitySet".to_string(),
                    generics: vec![],
                }),
                effect_row: Some(EffectRow {
                    effects: vec![
                        capability::CLOCK_NOW.to_string(),
                        capability::NET_CONNECT.to_string(),
                        capability::NET_BIND.to_string(),
                        capability::FS_READ.to_string(),
                        capability::FS_WRITE.to_string(),
                        capability::IO_STDIN.to_string(),
                        capability::IO_STDOUT.to_string(),
                        capability::IO_STDERR.to_string(),
                        capability::PROCESS_EXEC.to_string(),
                        capability::ENV_READ.to_string(),
                        capability::ENV_WRITE.to_string(),
                        capability::RANDOM_GENERATE.to_string(),
                        capability::LOG_EMIT.to_string(),
                        capability::TRACE_SPAN.to_string(),
                    ],
                }),
                capability_reqs: Some(CapabilityReqs {
                    caps: vec![
                        capability::CLOCK_NOW.to_string(),
                        capability::NET_CONNECT.to_string(),
                        capability::NET_BIND.to_string(),
                        capability::FS_READ.to_string(),
                        capability::FS_WRITE.to_string(),
                        capability::IO_STDIN.to_string(),
                        capability::IO_STDOUT.to_string(),
                        capability::IO_STDERR.to_string(),
                        capability::PROCESS_EXEC.to_string(),
                        capability::ENV_READ.to_string(),
                        capability::ENV_WRITE.to_string(),
                        capability::RANDOM_GENERATE.to_string(),
                        capability::LOG_EMIT.to_string(),
                        capability::TRACE_SPAN.to_string(),
                    ],
                }),
                contract_clauses: None,
            },
        ],
    };
    append_full_stdlib_modules(&mut reg);
    reg
}

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
        contract_clauses: None,
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
        contract_clauses: None,
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
        contract_clauses: None,
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
        contract_clauses: None,
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

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_core::semantic_graph::NodeKind;

    fn has_function_entry(id: &str) -> bool {
        let reg = v1_registry_with_functions();
        reg.entries
            .iter()
            .any(|e| e.id.0 == id && e.kind == NodeKind::Function)
    }

    fn has_capability_effect(id: &str) -> bool {
        let reg = v1_registry_with_functions();
        reg.entries.iter().any(|e| {
            e.id.0 == id
                && e.kind == NodeKind::Function
                && e.effect_row.is_some()
                && e.capability_reqs.is_some()
        })
    }

    fn has_contract_clauses(id: &str) -> bool {
        let reg = v1_registry_with_functions();
        reg.entries
            .iter()
            .any(|e| e.id.0 == id && e.contract_clauses.is_some())
    }

    // Wave 17B: ok_or contract clauses survive the dedup loop
    #[test]
    fn v1_ok_or_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.core.option.ok_or"),
            "std.core.option.ok_or must have contract_clauses (pre-loop entry required)"
        );
    }

    // A7: crypto pure function entries
    #[test]
    fn v1_contains_crypto_hash() {
        assert!(
            has_function_entry("std.crypto.hash"),
            "std.crypto.hash must be present"
        );
    }

    #[test]
    fn v1_contains_crypto_hmac() {
        assert!(
            has_function_entry("std.crypto.hmac"),
            "std.crypto.hmac must be present"
        );
    }

    #[test]
    fn v1_contains_crypto_constant_time_eq() {
        assert!(has_function_entry("std.crypto.constant_time_eq"));
    }

    // A7: encoding pure function entries
    #[test]
    fn v1_contains_encoding_base64_encode() {
        assert!(has_function_entry("std.encoding.base64_encode"));
    }

    #[test]
    fn v1_contains_encoding_base64_decode() {
        assert!(has_function_entry("std.encoding.base64_decode"));
    }

    #[test]
    fn v1_contains_encoding_hex_encode() {
        assert!(has_function_entry("std.encoding.hex_encode"));
    }

    #[test]
    fn v1_contains_encoding_hex_decode() {
        assert!(has_function_entry("std.encoding.hex_decode"));
    }

    // A7: json pure function entries
    #[test]
    fn v1_contains_json_parse() {
        assert!(has_function_entry("std.json.parse"));
    }

    #[test]
    fn v1_contains_json_stringify() {
        assert!(has_function_entry("std.json.stringify"));
    }

    // A7: numeric narrowing entries
    #[test]
    fn v1_contains_numeric_narrow_to_i32() {
        assert!(has_function_entry("std.numeric.narrow_to_i32"));
    }

    #[test]
    fn v1_contains_numeric_narrow_to_u32() {
        assert!(has_function_entry("std.numeric.narrow_to_u32"));
    }

    // A7: capability (effectful) entries for io
    #[test]
    fn v1_contains_io_read_with_effect() {
        assert!(
            has_capability_effect("std.io.read"),
            "std.io.read must have effect_row and capability_reqs"
        );
    }

    #[test]
    fn v1_contains_io_write_with_effect() {
        assert!(has_capability_effect("std.io.write"));
    }

    #[test]
    fn v1_contains_io_flush_with_effect() {
        assert!(has_capability_effect("std.io.flush"));
    }

    // A7: capability entries for fs
    #[test]
    fn v1_contains_fs_open_with_effect() {
        assert!(has_capability_effect("std.fs.open"));
    }

    #[test]
    fn v1_contains_fs_read_with_effect() {
        assert!(has_capability_effect("std.fs.read"));
    }

    #[test]
    fn v1_contains_fs_write_with_effect() {
        assert!(has_capability_effect("std.fs.write"));
    }

    // A7: capability entries for env
    #[test]
    fn v1_contains_env_get_with_effect() {
        assert!(has_capability_effect("std.env.get"));
    }

    #[test]
    fn v1_contains_env_set_with_effect() {
        assert!(has_capability_effect("std.env.set"));
    }

    // A7: capability entries for log and trace
    #[test]
    fn v1_contains_log_log_with_effect() {
        assert!(has_capability_effect("std.log.log"));
    }

    #[test]
    fn v1_contains_trace_span_with_effect() {
        assert!(has_capability_effect("std.trace.span"));
    }

    #[test]
    fn v1_contains_trace_event_with_effect() {
        assert!(has_capability_effect("std.trace.event"));
    }

    // Wave 21D: std.collections list/map/set contract clauses
    #[test]
    fn v1_list_length_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.collections.list.length"),
            "std.collections.list.length must have contract_clauses (pre-loop entry required)"
        );
    }

    #[test]
    fn v1_list_push_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.collections.list.push"),
            "std.collections.list.push must have contract_clauses"
        );
    }

    #[test]
    fn v1_list_get_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.collections.list.get"),
            "std.collections.list.get must have contract_clauses"
        );
    }

    #[test]
    fn v1_list_map_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.collections.list.map"),
            "std.collections.list.map must have contract_clauses"
        );
    }

    #[test]
    fn v1_list_filter_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.collections.list.filter"),
            "std.collections.list.filter must have contract_clauses"
        );
    }

    #[test]
    fn v1_list_fold_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.collections.list.fold"),
            "std.collections.list.fold must have contract_clauses"
        );
    }

    #[test]
    fn v1_list_concat_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.collections.list.concat"),
            "std.collections.list.concat must have contract_clauses"
        );
    }

    #[test]
    fn v1_map_get_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.collections.map.get"),
            "std.collections.map.get must have contract_clauses"
        );
    }

    #[test]
    fn v1_map_insert_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.collections.map.insert"),
            "std.collections.map.insert must have contract_clauses"
        );
    }

    #[test]
    fn v1_set_contains_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.collections.set.contains"),
            "std.collections.set.contains must have contract_clauses"
        );
    }

    #[test]
    fn v1_set_insert_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.collections.set.insert"),
            "std.collections.set.insert must have contract_clauses"
        );
    }

    // Wave 21D: std.time contract clauses
    #[test]
    fn v1_time_duration_since_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.time.duration_since"),
            "std.time.duration_since must have contract_clauses"
        );
    }

    #[test]
    fn v1_time_add_duration_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.time.add_duration"),
            "std.time.add_duration must have contract_clauses"
        );
    }

    #[test]
    fn v1_time_instant_to_ms_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.time.instant_to_ms"),
            "std.time.instant_to_ms must have contract_clauses"
        );
    }

    #[test]
    fn v1_time_now_has_contract_clauses() {
        assert!(
            has_contract_clauses("std.time.now"),
            "std.time.now must have contract_clauses (pre-loop entry required)"
        );
    }

    #[test]
    fn v1_time_now_has_capability_effect() {
        assert!(
            has_capability_effect("std.time.now"),
            "std.time.now must have effect_row and capability_reqs (clock.now)"
        );
    }

    // Wave 18C: text predicate entries
    #[test]
    fn v1_contains_text_starts_with() {
        assert!(
            has_function_entry("std.text.starts_with"),
            "std.text.starts_with must be present"
        );
    }

    #[test]
    fn v1_contains_text_ends_with() {
        assert!(
            has_function_entry("std.text.ends_with"),
            "std.text.ends_with must be present"
        );
    }

    #[test]
    fn v1_contains_text_contains() {
        assert!(
            has_function_entry("std.text.contains"),
            "std.text.contains must be present"
        );
    }

    #[test]
    fn v1_contains_text_replace() {
        assert!(
            has_function_entry("std.text.replace"),
            "std.text.replace must be present"
        );
    }
}
