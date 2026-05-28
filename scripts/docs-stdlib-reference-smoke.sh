#!/usr/bin/env bash
# Static smoke checks for the validation-stage stdlib reference.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$ROOT_DIR/docs/stdlib-reference.md"
LIB="$ROOT_DIR/crates/ail-stdlib/src/lib.rs"
REGISTRY="$ROOT_DIR/crates/ail-stdlib/src/registry.rs"
MODULE_ENTRIES="$ROOT_DIR/crates/ail-stdlib/src/v1/module_entries.rs"
FUNCTION_ENTRIES="$ROOT_DIR/crates/ail-stdlib/src/v1/function_entries.rs"
EXEC_REGISTRY="$ROOT_DIR/crates/ail-stdlib/src/exec/registry.rs"
CAPABILITY="$ROOT_DIR/crates/ail-stdlib/src/capability.rs"
COMPAT="$ROOT_DIR/docs/compatibility.md"

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
require_literal "$DOC" "not a production-readiness or Rust-level stdlib stability claim" "production caveat"
require_literal "$DOC" "Semantic Graph remains the source of truth" "semantic graph framing"
require_literal "$DOC" '`StabilityTier::Stable` is registry metadata' "stability caveat"
require_literal "$DOC" "Capability-backed descriptors are not ambient permissions" "capability framing"
require_literal "$DOC" "Compatibility policy" "compatibility link"

for module in std.core std.option std.result std.numeric std.decimal std.text std.bytes std.collections std.iter std.encoding std.json std.time std.random std.crypto std.io std.fs std.net std.http std.process std.env std.concurrent std.sync std.testing std.boundary std.diagnostics std.verify std.runtime std.capability; do
  require_literal "$DOC" "$module" "documented module $module"
done

for module in numeric decimal option result text bytes collections iter encoding json time random crypto io fs net http process env concurrent sync log trace testing boundary diagnostics verify runtime exec; do
  require_literal "$LIB" "pub mod $module;" "lib module $module"
done

for symbol in StdlibRegistry StabilityTier StdlibEntry StdlibId cbor_bytes "fn hash" to_graph_nodes DuplicateId; do
  require_literal "$REGISTRY" "$symbol" "registry evidence $symbol"
done

for id in std.core std.option std.result std.numeric std.text std.bytes std.collections std.iter std.capability std.decimal std.encoding std.json std.time std.random std.crypto std.io std.fs std.net std.http std.process std.env std.concurrent std.sync std.log std.trace std.testing std.boundary std.diagnostics std.verify std.runtime; do
  require_literal "$MODULE_ENTRIES" "$id" "module entry $id"
done

for id in std.numeric.checked_add std.numeric.narrow_to_i32 std.core.option.map std.core.option.ok_or std.core.result.transpose std.text.length_graphemes std.text.replace std.iter.traverse std.bytes.slice std.collections.list.fold std.collections.map.insert std.collections.set.insert std.time.now; do
  require_literal "$FUNCTION_ENTRIES" "$id" "function entry $id"
  require_literal "$DOC" "$id" "documented function $id"
done

for id in std.crypto.hash std.encoding.base64_encode std.json.parse std.concurrent.channel_new std.fs.read_file std.net.connect std.http.request std.process.spawn std.env.get std.log.log std.trace.span; do
  require_literal "$EXEC_REGISTRY" "$id" "exec entry $id"
  require_literal "$DOC" "$id" "documented exec entry $id"
done

for capability in clock.now file.read file.write network.connect http.call process.spawn env.read log.write trace.emit random.generate; do
  require_literal "$CAPABILITY" "$capability" "capability constant $capability"
  require_literal "$DOC" "$capability" "documented capability $capability"
done

require_literal "$COMPAT" "Stdlib APIs" "compatibility stdlib surface"

printf 'docs stdlib reference smoke passed\n'
