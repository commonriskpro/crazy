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
/// - `std.option`: `map`, `and_then`, `unwrap_or`, `transpose`,
///   `collect_results`
/// - `std.result`: `map`, `and_then`, `unwrap_or`, `transpose`
/// - `std.text`: `trim`, `split`, `join`, `length_graphemes`,
///   `to_bytes`, `from_bytes`
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

    // ── std.option functions ──────────────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.option.map".to_string()),
        module_path: "std::option".to_string(),
        name: "map".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.option.and_then".to_string()),
        module_path: "std::option".to_string(),
        name: "and_then".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.option.unwrap_or".to_string()),
        module_path: "std::option".to_string(),
        name: "unwrap_or".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "T".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.option.transpose".to_string()),
        module_path: "std::option".to_string(),
        name: "transpose".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["Option".to_string(), "T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.option.collect_results".to_string()),
        module_path: "std::option".to_string(),
        name: "collect_results".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["Vec".to_string(), "T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
    });

    // ── std.result functions ──────────────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.result.map".to_string()),
        module_path: "std::result".to_string(),
        name: "map".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["T".to_string(), "U".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.result.and_then".to_string()),
        module_path: "std::result".to_string(),
        name: "and_then".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["T".to_string(), "U".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.result.unwrap_or".to_string()),
        module_path: "std::result".to_string(),
        name: "unwrap_or".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "T".to_string(),
            generics: vec!["T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.result.transpose".to_string()),
        module_path: "std::result".to_string(),
        name: "transpose".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["Result".to_string(), "T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
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
}
