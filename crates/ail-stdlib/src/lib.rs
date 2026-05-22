// ── ail-stdlib ────────────────────────────────────────────────────────────
//
// Canonical data crate for the AIL v1 standard-library registry.
//
// # Public API
//
// | Module | Contents |
// |--------|----------|
// | `registry` | `StabilityTier`, `StdlibId`, `StdlibEntry`, `StdlibRegistry`, `StdlibError` |
// | `capability` | `pub const &str` capability name constants |
// | `v1` | `v1_registry()` — the extended v1 stdlib registry with function entries |
//
// # Dependency isolation
//
// `ail-stdlib` depends only on `ail-core`, `serde`, `ciborium`, `blake3`, and
// `unicode-segmentation`.
// It MUST NOT depend on `ail-verify`, `ail-compiler`, or `ail-runtime`.

/// Canonical capability name constants for `std.capability`.
pub mod capability;

/// Core registry types: `StabilityTier`, `StdlibId`, `StdlibEntry`,
/// `StdlibRegistry`, `StdlibError`, and all associated methods.
pub mod registry;

/// Checked, wrapping, saturating arithmetic, narrowing conversions, and
/// rounding policies (`std.numeric`).
pub mod numeric;

/// Fixed-point decimal arithmetic (`std.decimal`).
pub mod decimal;

/// `Option<T>` combinators (`std.option`).
pub mod option;

/// `Result<T, E>` combinators (`std.result`).
pub mod result;

/// Text helpers and grapheme-cluster counting (`std.text`).
pub mod text;

/// Byte buffer operations (`std.bytes`).
pub mod bytes;

/// Collection types: List, Set, Map, Queue, builders (`std.collections`).
pub mod collections;

/// Effect-polymorphic iterator combinators (`std.iter`).
pub mod iter;

/// Base64 and hex encoding/decoding (`std.encoding`).
pub mod encoding;

/// JSON parse and stringify (`std.json`).
pub mod json;

/// Timestamp, duration, and date/time types (`std.time`).
pub mod time;

/// Seeded random and crypto-random markers (`std.random`).
pub mod random;

/// Cryptographic primitives: Hash, Hmac, SecureBytes, ConstantTimeEq (`std.crypto`).
pub mod crypto;

/// Generic I/O traits and in-memory stream (`std.io`).
pub mod io;

/// Filesystem types: Path, FileError, FileResource, FsCapability (`std.fs`).
pub mod fs;

/// Network primitive types: Url, NetError, Timeout, RetryPolicy (`std.net`).
pub mod net;

/// HTTP client/server types: HttpRequest, HttpResponse, StatusCode (`std.http`).
pub mod http;

/// Process management types: ProcessHandle, ExitCode, Signal (`std.process`).
pub mod process;

/// Environment variable access: EnvVar, env_read, env_write (`std.env`).
pub mod env;

/// Task and channel concurrency primitives (`std.concurrent`).
pub mod concurrent;

/// Synchronization primitives: AilMutex, AilRwLock, AilAtomicBool/I64 (`std.sync`).
pub mod sync;

/// Structured logging: LogLevel, LogEvent, Logger, LogSink (`std.log`).
pub mod log;

/// Tracing and spans: TraceId, SpanId, Span, Metric (`std.trace`).
pub mod trace;

/// Testing helpers: assert_eq, assert_approx, expect_error, generate_cases_from_contract (`std.testing`).
pub mod testing;

/// Boundary/FFI helpers: BoundaryDef, AdapterContract, ForeignType, TrustLevel (`std.boundary`).
pub mod boundary;

/// Diagnostic types and helpers: format_diagnostic, extract_repair_ops, group_obligations (`std.diagnostics`).
pub mod diagnostics;

/// Verification helpers: VerificationReport, extract_repair_ops, group_obligations (`std.verify`).
pub mod verify;

/// Runtime-facing types: RuntimeProfile, LimitConfig, AuditEvent, ArtifactManifest (`std.runtime`).
pub mod runtime;

/// Canonical v1 stdlib module registry — `v1_registry()`.
pub mod v1;

pub use registry::{StabilityTier, StdlibEntry, StdlibError, StdlibId, StdlibRegistry};
pub use v1::{v1_registry, v1_registry_with_functions};
