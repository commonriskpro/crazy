#![no_main]

use ail_runtime::{CapabilityManifest, ResourceLimits, RuntimeHost, RuntimeProfile, blake3_hex_of};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Build a manifest with no required capabilities.
    let manifest = CapabilityManifest {
        module: "fuzz-target".to_string(),
        requires: vec![],
    };

    // Compute hashes from the fuzz input so preflight passes the hash checks
    // and Wasmtime validation is actually reached. This exercises the
    // structural WASM validator (and instantiation) on arbitrary bytes.
    let module_hash = blake3_hex_of(data);
    let manifest_hash = match manifest.blake3_hex() {
        Ok(h) => h,
        Err(_) => return,
    };

    let profile = RuntimeProfile::new(
        "fuzz-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    );

    let mut host = RuntimeHost::new();
    // All outcomes (Ok or Err) are expected — only panics are failures.
    let _ = host.validate_and_instantiate(data, &manifest, &profile);
});
