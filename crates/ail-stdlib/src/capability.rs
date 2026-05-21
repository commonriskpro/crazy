// ── ail-stdlib::capability ────────────────────────────────────────────────
//
// Canonical capability name constants for the `std.capability` stdlib module.
//
// Each constant is a lower-dotted string (lowercase ASCII + dots) compatible
// with `ail_core::semantic_graph::CapabilityReqs::caps` entries.
//
// These strings are also compatible with `CapabilityId::new(...)` in
// `ail-runtime`; however, `ail-stdlib` does NOT depend on `ail-runtime` —
// the string contract is enforced by convention and the test suite.

// ── Clock ─────────────────────────────────────────────────────────────────

/// Capability to read the current wall-clock time.
pub const CLOCK_NOW: &str = "clock.now";

// ── Network ───────────────────────────────────────────────────────────────

/// Capability to open outbound network connections.
pub const NET_CONNECT: &str = "net.connect";

/// Capability to bind a local address and accept inbound connections.
pub const NET_BIND: &str = "net.bind";

// ── Filesystem ────────────────────────────────────────────────────────────

/// Capability to read data from the filesystem.
pub const FS_READ: &str = "fs.read";

/// Capability to write data to the filesystem.
pub const FS_WRITE: &str = "fs.write";

// ── I/O streams ───────────────────────────────────────────────────────────

/// Capability to read from standard input.
pub const IO_STDIN: &str = "io.stdin";

/// Capability to write to standard output.
pub const IO_STDOUT: &str = "io.stdout";

/// Capability to write to standard error.
pub const IO_STDERR: &str = "io.stderr";

// ── Process ───────────────────────────────────────────────────────────────

/// Capability to spawn or execute child processes.
pub const PROCESS_EXEC: &str = "process.exec";

// ── Environment ───────────────────────────────────────────────────────────

/// Capability to read environment variables.
pub const ENV_READ: &str = "env.read";

/// Capability to write (set) environment variables.
pub const ENV_WRITE: &str = "env.write";

// ── Randomness ────────────────────────────────────────────────────────────

/// Capability to generate random bytes.
pub const RANDOM_GENERATE: &str = "random.generate";

// ── Observability ─────────────────────────────────────────────────────────

/// Capability to emit structured log events.
pub const LOG_EMIT: &str = "log.emit";

/// Capability to create and manage trace spans.
pub const TRACE_SPAN: &str = "trace.span";
