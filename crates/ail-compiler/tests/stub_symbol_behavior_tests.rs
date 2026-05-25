// ── ail-compiler::stub_symbol_behavior_tests ─────────────────────────────
//
// Verifies the structural and behavioral contract of the generated runtime
// stub archive and the object file it contains.
//
// # Honest scope declaration
//
// These tests are pure-Rust and do not execute the stub functions.  What they
// prove:
//
//   B1 — build_runtime_stub_object() returns bytes with the correct
//        platform-native object file magic (ELF / Mach-O / COFF).
//   B2 — The ar archive member-header size field matches the actual object
//        byte count embedded in the archive.
//   B3 — The object bytes extracted from the archive carry the same platform-
//        native magic as build_runtime_stub_object() alone.
//   B4 — Both the object and the archive are byte-identical across repeated
//        calls (determinism).
//   B5 — The stub object exceeds the minimum size threshold expected from
//        trivially empty functions, suggesting real instruction bytes were
//        emitted for all three stubs.
//        (Structural proxy only — trap opcode presence is not directly verified.)
//
// # Symbol behavior contracts (not runtime-checked here)
//
// The following contracts are enforced by the generator in `native_stub.rs`
// and are documented here as the authoritative reference:
//
//   host_call(i64×6) → i64
//     Returns -1 immediately (no-op denial).  Safe to call in programs that
//     dispatch capabilities — the call returns a sentinel without side effects.
//
//   __ail_malloc(i64) → !
//     Emits `trap(user(1))` — does NOT return.  Any linked binary that reaches
//     this symbol at runtime halts immediately at the call site with a
//     diagnosable hardware trap (SIGTRAP on Linux, EXC_BAD_INSTRUCTION on
//     macOS) instead of receiving a null pointer and segfaulting silently
//     later.  Stack traces point to the exact allocation site.
//
//   ail_runtime_call(i64×3) → i64
//     Returns -1 immediately (no-op denial).  Concurrency/channel/resource
//     primitives that dispatch here will see a -1 sentinel; the program
//     continues and any error handling upstream can observe the value.
//
// # Safe programs
//
// A native binary linked against the stub archive can safely run if it never
// reaches a heap-allocation path (`__ail_malloc`) at runtime.  Specifically:
//   - Pure arithmetic / control-flow programs: fully safe.
//   - Programs with EffectCall (host_call): return -1, observable as an error.
//   - Programs with TaskSpawn / Channel / Resource (ail_runtime_call): -1
//     sentinel; upstream error handling governs behaviour.
//   - Programs that allocate heap objects (records/variants/lists on heap):
//     will trap immediately at the first allocation site — diagnosable.

use ail_compiler::{build_runtime_stub_archive, build_runtime_stub_object};

// ── helpers ───────────────────────────────────────────────────────────────

/// Expected object file magic bytes for the current platform.
fn native_object_magic() -> &'static [u8] {
    #[cfg(target_os = "linux")]
    return &[0x7F, 0x45, 0x4C, 0x46]; // ELF

    #[cfg(target_os = "macos")]
    return &[0xCF, 0xFA, 0xED, 0xFE]; // Mach-O 64-bit LE

    #[cfg(target_os = "windows")]
    return &[]; // COFF has no universal magic; accept non-empty

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return &[];
}

fn assert_native_object_magic(bytes: &[u8], label: &str) {
    let magic = native_object_magic();
    if magic.is_empty() {
        assert!(!bytes.is_empty(), "{label}: object bytes must be non-empty");
        return;
    }
    assert!(
        bytes.len() >= magic.len(),
        "{label}: object too short to contain magic ({} bytes)",
        bytes.len()
    );
    assert_eq!(
        &bytes[..magic.len()],
        magic,
        "{label}: magic header mismatch — expected {magic:02X?}, got {:02X?}",
        &bytes[..magic.len()]
    );
}

/// Extract the embedded object bytes from a stub ar archive.
///
/// ar layout:
///   bytes  0.. 7: global header `!<arch>\n`
///   bytes  8..23: member name (16 bytes)
///   bytes 24..35: mtime (12 bytes)
///   bytes 36..41: uid (6 bytes)
///   bytes 42..47: gid (6 bytes)
///   bytes 48..55: mode (8 bytes)
///   bytes 56..65: size decimal, space-padded to 10 bytes
///   bytes 66..67: end marker `\`\n`
///   bytes 68..  : object data (size bytes)
fn extract_object_from_archive(archive: &[u8]) -> &[u8] {
    assert!(
        archive.len() >= 68,
        "archive too short to contain member header ({} bytes)",
        archive.len()
    );
    assert_eq!(
        &archive[..8],
        b"!<arch>\n",
        "archive must start with ar magic"
    );

    let size_field = std::str::from_utf8(&archive[56..66])
        .expect("size field must be valid UTF-8")
        .trim();
    let size: usize = size_field
        .parse()
        .unwrap_or_else(|e| panic!("size field {size_field:?} is not a valid integer: {e}"));

    assert!(
        archive.len() >= 68 + size,
        "archive too short: header claims {size} bytes of object data but archive is only {} bytes",
        archive.len()
    );
    &archive[68..68 + size]
}

// ── B1 — stub object has platform-native magic ────────────────────────────

/// Stub symbol behavior B1: build_runtime_stub_object() emits bytes that
/// start with the platform-native object file magic.
///
/// This is the primary structural assertion: the stub is a PLATFORM-NATIVE
/// OBJECT FILE, not raw machine code or a script.
#[test]
fn stub_object_has_platform_native_magic() {
    let obj = build_runtime_stub_object().expect("build_runtime_stub_object must succeed");
    assert_native_object_magic(&obj, "stub_object_has_platform_native_magic");
}

/// Stub symbol behavior B1b: stub object is non-trivially large.
///
/// An object with three function bodies (including one trap stub) must exceed
/// the bare file header in size.
#[test]
fn stub_object_has_non_trivial_size() {
    let obj = build_runtime_stub_object().expect("build_runtime_stub_object must succeed");
    // Even the smallest Mach-O/ELF header + 3 function stubs is well over 64 bytes.
    assert!(
        obj.len() > 64,
        "stub object must exceed 64 bytes; got {} bytes",
        obj.len()
    );
}

// ── B2 — archive size field matches embedded object ───────────────────────

/// Stub symbol behavior B2: the member-header size field in the ar archive
/// matches the actual byte count of the embedded object file.
///
/// This verifies the archive writer emits a well-formed member header.
#[test]
fn stub_archive_member_size_field_matches_embedded_object_length() {
    let archive = build_runtime_stub_archive().expect("build_runtime_stub_archive must succeed");
    let obj_from_archive = extract_object_from_archive(&archive);
    let obj_standalone =
        build_runtime_stub_object().expect("build_runtime_stub_object must succeed");

    assert_eq!(
        obj_from_archive.len(),
        obj_standalone.len(),
        "archive size field must match standalone object length"
    );
}

// ── B3 — extracted object has platform-native magic ───────────────────────

/// Stub symbol behavior B3: the object file extracted from the archive starts
/// with the correct platform-native magic bytes.
///
/// Verifies that wrap_in_ar_archive does not corrupt the embedded object file.
#[test]
fn stub_archive_embedded_object_has_platform_native_magic() {
    let archive = build_runtime_stub_archive().expect("build_runtime_stub_archive must succeed");
    let embedded = extract_object_from_archive(&archive);
    assert_native_object_magic(
        embedded,
        "stub_archive_embedded_object_has_platform_native_magic",
    );
}

/// Stub symbol behavior B3b: the bytes extracted from the archive are
/// byte-identical to those produced by build_runtime_stub_object() alone.
#[test]
fn stub_archive_embedded_object_matches_standalone_object() {
    let archive = build_runtime_stub_archive().expect("build_runtime_stub_archive must succeed");
    let embedded = extract_object_from_archive(&archive);
    let standalone = build_runtime_stub_object().expect("build_runtime_stub_object must succeed");

    assert_eq!(
        embedded,
        standalone.as_slice(),
        "object embedded in archive must be byte-identical to standalone build_runtime_stub_object()"
    );
}

// ── B4 — determinism ──────────────────────────────────────────────────────

/// Stub symbol behavior B4: build_runtime_stub_object() is deterministic.
///
/// Same ISA → byte-identical object on every call.  Required for reproducible
/// build evidence and archive caching.
#[test]
fn stub_object_is_deterministic() {
    let a = build_runtime_stub_object().expect("first call");
    let b = build_runtime_stub_object().expect("second call");
    assert_eq!(
        a, b,
        "build_runtime_stub_object must produce byte-identical output on repeated calls"
    );
}

/// Stub symbol behavior B4b: build_runtime_stub_archive() is deterministic.
#[test]
fn stub_archive_is_deterministic() {
    let a = build_runtime_stub_archive().expect("first call");
    let b = build_runtime_stub_archive().expect("second call");
    assert_eq!(
        a, b,
        "build_runtime_stub_archive must produce byte-identical output on repeated calls"
    );
}

// ── B5 — stub has non-trivial size (structural proxy for real bodies) ──────

/// Stub symbol behavior B5: the stub object exceeds the minimum size expected
/// from trivially empty functions.
///
/// This is a STRUCTURAL PROXY, not proof of specific opcodes.  A size
/// threshold above what three no-op stubs would produce is consistent with
/// real instruction bytes having been emitted, but does not directly verify
/// that a trap opcode is present in the __ail_malloc body.
#[test]
fn stub_object_has_real_function_bodies_not_empty_stubs() {
    let obj = build_runtime_stub_object().expect("build_runtime_stub_object must succeed");
    // Three stubs with real instructions must produce at least 128 bytes of
    // object data beyond the bare file header.  A 64-byte minimum was set in
    // stub_object_has_non_trivial_size; 128 is the triangulation point.
    assert!(
        obj.len() > 128,
        "stub object must exceed 128 bytes (real function bodies); got {} bytes",
        obj.len()
    );
}

/// Stub symbol behavior B5b: the archive size is consistent with a non-trivial
/// object (header + at least the object + padding).
#[test]
fn stub_archive_total_size_is_consistent_with_non_trivial_object() {
    let archive = build_runtime_stub_archive().expect("build_runtime_stub_archive must succeed");
    let obj = build_runtime_stub_object().expect("build_runtime_stub_object must succeed");

    // archive = 8 (global) + 60 (member header) + obj.len() + 0/1 (padding)
    let expected_min = 8 + 60 + obj.len();
    let expected_max = expected_min + 1; // optional \n padding

    assert!(
        archive.len() >= expected_min && archive.len() <= expected_max,
        "archive length {} must be in [{expected_min}, {expected_max}]",
        archive.len()
    );
}
