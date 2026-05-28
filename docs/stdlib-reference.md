# AIL stdlib reference

<!-- Status: Implemented subset. This reference documents the current `ail-stdlib` registry, function descriptors, and capability names backed by source evidence. It is not a production-readiness or Rust-level stdlib stability claim. -->

AIL's standard library is represented as Semantic Graph metadata plus executable function descriptors. The implemented subset is useful for validation, but the broader stdlib design in [Standard library shape](stdlib.md) is still larger than what users should treat as stable product surface.

## Quick path

1. Use this document to see what `ail-stdlib` currently exposes.
2. Use [Compatibility policy](compatibility.md) before changing stdlib IDs, capability names, function descriptors, or contracts.
3. Keep effects explicit: stdlib may define capabilities, but runtime grants still control execution.

## Current implementation model

| Layer | Implemented evidence | What it means |
|---|---|---|
| Registry metadata | `crates/ail-stdlib/src/registry.rs` | Ordered `StdlibEntry` records with deterministic CBOR/hash behavior and Semantic Graph projection. |
| v1 module registry | `crates/ail-stdlib/src/v1/module_entries.rs` | Canonical v1 module entries with `StabilityTier::Stable` metadata. |
| v1 function registry | `crates/ail-stdlib/src/v1/function_entries.rs` | Function entries for numeric, option/result, text, iter, bytes, collections, time, and capability-backed functions. |
| Executable descriptors | `crates/ail-stdlib/src/exec/registry.rs` | Pure and capability-backed dispatch descriptors. |
| Capability names | `crates/ail-stdlib/src/capability.rs` | Canonical capability constants such as `log.write`, `file.read`, `network.connect`, and `clock.now`. |

`StabilityTier::Stable` is registry metadata. Because AIL itself is still v0.x validation-stage, do not translate that into a production-ready compatibility promise without release evidence.

## Registry guarantees

The registry implementation currently guarantees:

- deterministic entry order;
- duplicate `StdlibId` validation;
- deterministic CBOR bytes;
- BLAKE3 registry hash;
- projection to Semantic Graph nodes with deterministic `NodeRef(index)` assignment;
- no dependency on verifier, compiler, or runtime crates from `ail-stdlib`.

These properties matter because Semantic Graph remains the source of truth. Stdlib entries are not just helper functions; they become graph-visible semantic facts, effects, capabilities, and contracts.

## Implemented module entries

The v1 registry includes base modules plus broader module descriptors:

| Category | Modules |
|---|---|
| Core/data | `std.core`, `std.option`, `std.result`, `std.numeric`, `std.decimal`, `std.text`, `std.bytes`, `std.collections`, `std.iter` |
| Encoding/data formats | `std.encoding`, `std.json` |
| Effects/capabilities | `std.time`, `std.random`, `std.crypto`, `std.io`, `std.fs`, `std.net`, `std.http`, `std.process`, `std.env`, `std.log`, `std.trace` |
| Concurrency/runtime/tooling | `std.concurrent`, `std.sync`, `std.testing`, `std.boundary`, `std.diagnostics`, `std.verify`, `std.runtime`, `std.capability` |

Capability-backed modules carry effect/capability metadata. Pure modules should not smuggle ambient effects.

## Implemented pure function families

| Family | Implemented functions |
|---|---|
| Numeric | `std.numeric.checked_add`, `std.numeric.checked_sub`, `std.numeric.checked_mul`, `std.numeric.wrapping_add`, `std.numeric.saturating_add`, `std.numeric.narrow_to_i32`, `std.numeric.narrow_to_u32` |
| Option | `std.core.option.map`, `std.core.option.and_then`, `std.core.option.unwrap_or`, `std.core.option.transpose`, `std.core.option.collect_results`, `std.core.option.ok_or` |
| Result | `std.core.result.map`, `std.core.result.and_then`, `std.core.result.unwrap_or`, `std.core.result.transpose` |
| Text | `std.text.trim`, `std.text.split`, `std.text.join`, `std.text.normalize`, `std.text.encode`, `std.text.decode`, `std.text.format`, `std.text.regex`, `std.text.length_graphemes`, `std.text.starts_with`, `std.text.ends_with`, `std.text.contains`, `std.text.replace` |
| Bytes | `std.bytes.length`, `std.bytes.at`, `std.bytes.slice`, `std.bytes.concat`, `std.bytes.empty` |
| Collections | `std.collections.list.length`, `std.collections.list.push`, `std.collections.list.get`, `std.collections.list.map`, `std.collections.list.filter`, `std.collections.list.fold`, `std.collections.list.concat`, `std.collections.map.get`, `std.collections.map.insert`, `std.collections.set.contains`, `std.collections.set.insert` |
| Iteration | `std.iter.map`, `std.iter.filter`, `std.iter.fold`, `std.iter.traverse` |
| Encoding/JSON/Crypto | `std.encoding.base64_encode`, `std.encoding.base64_decode`, `std.encoding.hex_encode`, `std.encoding.hex_decode`, `std.json.parse`, `std.json.stringify`, `std.crypto.hash`, `std.crypto.hmac`, `std.crypto.constant_time_eq` |
| Concurrent/time pure helpers | `std.concurrent.channel_new`, `std.concurrent.channel_send`, `std.concurrent.channel_recv`, `std.concurrent.channel_len`, `std.time.duration_since`, `std.time.add_duration`, `std.time.instant_to_ms` |

## Capability-backed function descriptors

Capability-backed descriptors are not ambient permissions. They name the capability a runtime host must grant.

| Function | Capability |
|---|---|
| `std.time.now` | `clock.now` |
| `std.random.next_int`, `std.random.next_float` | `random.int`, `random.float` |
| `std.io.read` | `io.stdin` |
| `std.io.write`, `std.io.flush` | `io.stdout` |
| `std.io.seek` | `io.seek` |
| `std.fs.open`, `std.fs.read`, `std.fs.read_file`, `std.fs.stat` | `file.read` |
| `std.fs.write` | `file.write` |
| `std.fs.delete` | `file.delete` |
| `std.fs.list` | `file.list` |
| `std.net.connect`, `std.net.send`, `std.net.receive` | `network.connect` |
| `std.net.listen` | `network.bind` |
| `std.http.request` | `http.call` |
| `std.http.serve` | `http.serve` |
| `std.process.spawn` | `process.spawn` |
| `std.process.wait` | `process.wait` |
| `std.process.kill` | `process.signal` |
| `std.env.get`, `std.env.list` | `env.read` |
| `std.env.set` | `env.write` |
| `std.log.log` | `log.write` |
| `std.trace.span`, `std.trace.event` | `trace.emit` |

## Capability constants

`crates/ail-stdlib/src/capability.rs` currently defines canonical or exec-facing names including:

```txt
clock.now
fs.read
fs.write
file.read
file.write
file.delete
file.list
network.connect
network.bind
http.call
http.serve
process.spawn
process.wait
process.signal
env.read
env.write
io.stdin
io.stdout
io.stderr
log.write
trace.emit
random.generate
```

Changing any capability string is compatibility-sensitive. Use [Compatibility policy](compatibility.md), update release notes, and provide migration guidance.

## What is not product-grade yet

Do not overclaim the stdlib. Current gaps include:

- no full public stability promise for v0.x stdlib APIs;
- incomplete examples for every stdlib item;
- limited package/ecosystem integration;
- no broad compatibility fixture matrix for old stdlib IDs or contracts;
- capability-backed descriptors still need production host/runtime hardening evidence;
- docs describe more target design than the executable product experience currently proves.

## Review checklist

Before changing stdlib surface, verify:

- [ ] `StdlibId` changes are classified with [Compatibility policy](compatibility.md).
- [ ] Capability string changes are treated as compatibility-sensitive.
- [ ] Function descriptor changes update this reference and smoke checks.
- [ ] Contract/effect/capability metadata changes preserve explicit effects.
- [ ] User-visible changes are noted in `CHANGELOG.md`.
