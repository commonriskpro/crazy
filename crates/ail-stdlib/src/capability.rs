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

// ── Exec-facing aliases ───────────────────────────────────────────────────
//
// exec.rs uses different naming conventions than the fs/net/process constants
// above (which follow the canonical capability system naming).  These aliases
// match exec.rs usage exactly so exec entries can reference constants rather
// than raw strings.  Both old and new constants are kept; no renaming.

/// File-read capability as used by exec.rs entries (matches exec.rs "file.read").
pub const FILE_READ: &str = "file.read";

/// File-write capability as used by exec.rs entries.
pub const FILE_WRITE: &str = "file.write";

/// File-delete capability as used by exec.rs entries.
pub const FILE_DELETE: &str = "file.delete";

/// File-list capability as used by exec.rs entries.
pub const FILE_LIST: &str = "file.list";

/// Outbound network-connect capability as used by exec.rs entries.
pub const NETWORK_CONNECT: &str = "network.connect";

/// Inbound network-bind capability as used by exec.rs entries.
pub const NETWORK_BIND: &str = "network.bind";

/// HTTP outbound call capability.
pub const HTTP_CALL: &str = "http.call";

/// HTTP server capability.
pub const HTTP_SERVE: &str = "http.serve";

/// Process spawn capability.
pub const PROCESS_SPAWN: &str = "process.spawn";

/// Process wait capability.
pub const PROCESS_WAIT: &str = "process.wait";

/// Process signal (kill) capability.
pub const PROCESS_SIGNAL: &str = "process.signal";

/// Log write capability as used by exec.rs (matches exec.rs "log.write").
pub const LOG_WRITE: &str = "log.write";

/// Trace emit capability as used by exec.rs (matches exec.rs "trace.emit").
pub const TRACE_EMIT: &str = "trace.emit";

/// I/O stdin capability name used by exec.rs entries (same value as IO_STDIN).
pub const IO_STDIN_READ: &str = "io.stdin";

/// I/O stdout capability name used by exec.rs entries (same value as IO_STDOUT).
pub const IO_STDOUT_WRITE: &str = "io.stdout";

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // A3: Verify that exec.rs-matching alias constants have the correct values.
    // These tests reference constants that must be added in A4.

    // file.* aliases (exec.rs uses "file.read", not "fs.read")
    #[test]
    fn file_read_constant_matches_exec() {
        assert_eq!(FILE_READ, "file.read");
    }

    #[test]
    fn file_write_constant_matches_exec() {
        assert_eq!(FILE_WRITE, "file.write");
    }

    #[test]
    fn file_delete_constant_matches_exec() {
        assert_eq!(FILE_DELETE, "file.delete");
    }

    #[test]
    fn file_list_constant_matches_exec() {
        assert_eq!(FILE_LIST, "file.list");
    }

    // network.* aliases
    #[test]
    fn network_connect_constant_matches_exec() {
        assert_eq!(NETWORK_CONNECT, "network.connect");
    }

    #[test]
    fn network_bind_constant_matches_exec() {
        assert_eq!(NETWORK_BIND, "network.bind");
    }

    // http.* aliases
    #[test]
    fn http_call_constant_matches_exec() {
        assert_eq!(HTTP_CALL, "http.call");
    }

    #[test]
    fn http_serve_constant_matches_exec() {
        assert_eq!(HTTP_SERVE, "http.serve");
    }

    // process.* aliases
    #[test]
    fn process_spawn_constant_matches_exec() {
        assert_eq!(PROCESS_SPAWN, "process.spawn");
    }

    #[test]
    fn process_wait_constant_matches_exec() {
        assert_eq!(PROCESS_WAIT, "process.wait");
    }

    #[test]
    fn process_signal_constant_matches_exec() {
        assert_eq!(PROCESS_SIGNAL, "process.signal");
    }

    // log.write alias
    #[test]
    fn log_write_constant_matches_exec() {
        assert_eq!(LOG_WRITE, "log.write");
    }

    // trace.emit alias
    #[test]
    fn trace_emit_constant_matches_exec() {
        assert_eq!(TRACE_EMIT, "trace.emit");
    }

    // io.* aliases
    #[test]
    fn io_stdin_read_constant_matches_exec() {
        assert_eq!(IO_STDIN_READ, "io.stdin");
    }

    #[test]
    fn io_stdout_write_constant_matches_exec() {
        assert_eq!(IO_STDOUT_WRITE, "io.stdout");
    }

    // Verify pre-existing constants are unchanged
    #[test]
    fn pre_existing_constants_unchanged() {
        assert_eq!(CLOCK_NOW, "clock.now");
        assert_eq!(ENV_READ, "env.read");
        assert_eq!(ENV_WRITE, "env.write");
        assert_eq!(IO_STDIN, "io.stdin");
        assert_eq!(IO_STDOUT, "io.stdout");
    }
}
