// ── ail-cli::store_artifacts ──────────────────────────────────────────────
//
// Artifact persistence types and `StoreHandle` methods for WASM and native
// compiled artifacts.
//
// Extracted from store.rs in Phase 6.  `StoreHandle` is imported from
// `crate::store`; this module must NOT be imported by `store.rs` to avoid
// circular dependencies.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::CliError;
use crate::store::{StoreHandle, atomic_write, is_object_file_name};

// ── WASM artifact persistence types ──────────────────────────────────────

/// Raw bytes passed to `save_wasm_artifact`.
///
/// Groups the baseline sidecar payloads so the method signature stays under the
/// clippy `too_many_arguments` threshold.
pub struct WasmArtifactBytes<'a> {
    /// Compiled WASM module bytes.
    pub wasm: &'a [u8],
    /// Source map JSON bytes.
    pub source_map_json: &'a [u8],
    /// Artifact manifest JSON bytes.
    pub artifact_manifest_json: &'a [u8],
    /// Capabilities manifest JSON bytes.
    pub capabilities_manifest_json: &'a [u8],
}

/// Index entry recorded in `.ail/wasm/artifact-index.json`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WasmArtifactIndexEntry {
    /// Blake3 hex hash of the raw WASM bytes (64-char lowercase hex).
    pub hash: String,
    /// Compilation profile (e.g. `"dev"`, `"prod"`).
    pub profile: String,
    /// Compilation target (e.g. `"wasm"`).
    pub target: String,
    /// Unix epoch seconds when the artifact was persisted.
    pub stored_at: u64,
}

/// On-disk paths for the sidecar files written by WASM artifact persistence.
pub struct WasmArtifactPaths {
    pub wasm_path: PathBuf,
    pub source_map_path: PathBuf,
    pub manifest_path: PathBuf,
    pub capabilities_path: PathBuf,
    pub abi_descriptor_path: Option<PathBuf>,
}

/// A fully loaded persisted WASM artifact.
pub struct PersistedWasmArtifact {
    /// Blake3 hex hash of the raw WASM bytes.
    pub hash: String,
    /// Compilation profile.
    pub profile: String,
    /// Compilation target.
    pub target: String,
    /// Raw WASM bytes.
    pub wasm_bytes: Vec<u8>,
    /// Source map JSON bytes.
    pub source_map_json: Vec<u8>,
    /// Artifact manifest JSON bytes.
    pub artifact_manifest_json: Vec<u8>,
    /// Capabilities manifest JSON bytes.
    pub capabilities_manifest_json: Vec<u8>,
    /// ABI descriptor JSON bytes, when persisted by newer compilers.
    pub abi_descriptor_json: Option<Vec<u8>>,
    /// On-disk file paths.
    pub paths: WasmArtifactPaths,
}

// ── Native artifact persistence types ─────────────────────────────────────

/// Raw bytes passed to `save_native_artifact`.
///
/// Groups the four sidecar payloads so the method signature stays under the
/// clippy `too_many_arguments` threshold.
pub struct NativeArtifactBytes<'a> {
    /// Compiled native object bytes (ELF / Mach-O / COFF).
    pub object: &'a [u8],
    /// Source map JSON bytes.
    pub source_map_json: &'a [u8],
    /// Artifact manifest JSON bytes.
    pub artifact_manifest_json: &'a [u8],
    /// Capabilities manifest JSON bytes.
    pub capabilities_manifest_json: &'a [u8],
}

/// Index entry recorded in `.ail/native/artifact-index.json`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NativeArtifactIndexEntry {
    /// Blake3 hex hash of the raw native object bytes (64-char lowercase hex).
    pub hash: String,
    /// Compilation profile (e.g. `"dev"`, `"prod"`).
    pub profile: String,
    /// Compilation target (e.g. `"native"`).
    pub target: String,
    /// Unix epoch seconds when the artifact was persisted.
    pub stored_at: u64,
}

/// On-disk paths for the four sidecar files written by `save_native_artifact`.
pub struct NativeArtifactPaths {
    pub object_path: PathBuf,
    pub source_map_path: PathBuf,
    pub manifest_path: PathBuf,
    pub capabilities_path: PathBuf,
}

/// A fully loaded persisted native artifact.
pub struct PersistedNativeArtifact {
    /// Blake3 hex hash of the raw native object bytes.
    pub hash: String,
    /// Compilation profile.
    pub profile: String,
    /// Compilation target.
    pub target: String,
    /// Raw native object bytes.
    pub object_bytes: Vec<u8>,
    /// Source map JSON bytes.
    pub source_map_json: Vec<u8>,
    /// Artifact manifest JSON bytes.
    pub artifact_manifest_json: Vec<u8>,
    /// Capabilities manifest JSON bytes.
    pub capabilities_manifest_json: Vec<u8>,
    /// On-disk file paths.
    pub paths: NativeArtifactPaths,
}

// ── StoreHandle impl — artifact persistence ───────────────────────────────

impl StoreHandle {
    /// Persist a compiled WASM artifact and its sidecars under `.ail/wasm/`.
    ///
    /// Writes four baseline files keyed by `hash`:
    ///   - `<hash>.wasm`              — raw WASM bytes
    ///   - `<hash>.source_map.json`   — source map JSON
    ///   - `<hash>.manifest.json`     — artifact manifest JSON
    ///   - `<hash>.capabilities.json` — capabilities manifest JSON
    ///
    /// Also updates `.ail/wasm/artifact-index.json` with the new entry so
    /// `load_wasm_artifact` can find the artifact by name or hash later.
    ///
    /// Returns `Ok(Some(paths))` for file-backed stores.
    /// Returns `Ok(None)` for in-memory and Postgres backends (no-op).
    pub fn save_wasm_artifact(
        &self,
        hash: &str,
        profile: &str,
        target: &str,
        bytes: WasmArtifactBytes<'_>,
    ) -> Result<Option<WasmArtifactPaths>, CliError> {
        self.save_wasm_artifact_impl(hash, profile, target, bytes, None)
    }

    /// Persist a compiled WASM artifact plus its versioned ABI descriptor.
    ///
    /// In addition to the baseline files written by [`Self::save_wasm_artifact`],
    /// writes `<hash>.abi.json` so runtime/editor/package tooling can recover
    /// the export ABI contract without recompiling source.
    pub fn save_wasm_artifact_with_abi(
        &self,
        hash: &str,
        profile: &str,
        target: &str,
        bytes: WasmArtifactBytes<'_>,
        abi_descriptor_json: &[u8],
    ) -> Result<Option<WasmArtifactPaths>, CliError> {
        self.save_wasm_artifact_impl(hash, profile, target, bytes, Some(abi_descriptor_json))
    }

    fn save_wasm_artifact_impl(
        &self,
        hash: &str,
        profile: &str,
        target: &str,
        bytes: WasmArtifactBytes<'_>,
        abi_descriptor_json: Option<&[u8]>,
    ) -> Result<Option<WasmArtifactPaths>, CliError> {
        let StoreHandle::File { ail_dir, .. } = self else {
            return Ok(None);
        };
        let dir = wasm_dir(ail_dir);
        std::fs::create_dir_all(&dir)?;

        let wasm_path = dir.join(format!("{hash}.wasm"));
        let source_map_path = dir.join(format!("{hash}.source_map.json"));
        let manifest_path = dir.join(format!("{hash}.manifest.json"));
        let capabilities_path = dir.join(format!("{hash}.capabilities.json"));
        let abi_descriptor_path = abi_descriptor_json.map(|_| dir.join(format!("{hash}.abi.json")));

        atomic_write(&wasm_path, bytes.wasm)?;
        atomic_write(&source_map_path, bytes.source_map_json)?;
        atomic_write(&manifest_path, bytes.artifact_manifest_json)?;
        atomic_write(&capabilities_path, bytes.capabilities_manifest_json)?;
        if let (Some(path), Some(json)) = (&abi_descriptor_path, abi_descriptor_json) {
            atomic_write(path, json)?;
        }

        // Update the artifact index.
        let mut entries = read_wasm_artifact_index(ail_dir).unwrap_or_default();
        let stored_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Some(existing) = entries.iter_mut().find(|e| e.hash == hash) {
            existing.stored_at = stored_at;
            existing.profile = profile.to_string();
            existing.target = target.to_string();
        } else {
            entries.push(WasmArtifactIndexEntry {
                hash: hash.to_string(),
                profile: profile.to_string(),
                target: target.to_string(),
                stored_at,
            });
        }
        write_wasm_artifact_index(ail_dir, &entries)?;

        Ok(Some(WasmArtifactPaths {
            wasm_path,
            source_map_path,
            manifest_path,
            capabilities_path,
            abi_descriptor_path,
        }))
    }

    /// Persist a compiled native artifact and its sidecars under `.ail/native/`.
    ///
    /// Writes four files keyed by `hash`:
    ///   - `<hash>.o`                  — raw native object bytes
    ///   - `<hash>.source_map.json`    — source map JSON
    ///   - `<hash>.manifest.json`      — artifact manifest JSON
    ///   - `<hash>.capabilities.json`  — capabilities manifest JSON
    ///
    /// Also updates `.ail/native/artifact-index.json` with the new entry so
    /// `load_native_artifact` can find the artifact by name or hash later.
    ///
    /// Returns `Ok(Some(paths))` for file-backed stores.
    /// Returns `Ok(None)` for in-memory and Postgres backends (no-op).
    pub fn save_native_artifact(
        &self,
        hash: &str,
        profile: &str,
        target: &str,
        bytes: NativeArtifactBytes<'_>,
    ) -> Result<Option<NativeArtifactPaths>, CliError> {
        let StoreHandle::File { ail_dir, .. } = self else {
            return Ok(None);
        };
        let dir = native_dir(ail_dir);
        std::fs::create_dir_all(&dir)?;

        let object_path = dir.join(format!("{hash}.o"));
        let source_map_path = dir.join(format!("{hash}.source_map.json"));
        let manifest_path = dir.join(format!("{hash}.manifest.json"));
        let capabilities_path = dir.join(format!("{hash}.capabilities.json"));

        atomic_write(&object_path, bytes.object)?;
        atomic_write(&source_map_path, bytes.source_map_json)?;
        atomic_write(&manifest_path, bytes.artifact_manifest_json)?;
        atomic_write(&capabilities_path, bytes.capabilities_manifest_json)?;

        // Update the artifact index.
        let mut entries = read_native_artifact_index(ail_dir).unwrap_or_default();
        let stored_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Some(existing) = entries.iter_mut().find(|e| e.hash == hash) {
            existing.stored_at = stored_at;
            existing.profile = profile.to_string();
            existing.target = target.to_string();
        } else {
            entries.push(NativeArtifactIndexEntry {
                hash: hash.to_string(),
                profile: profile.to_string(),
                target: target.to_string(),
                stored_at,
            });
        }
        write_native_artifact_index(ail_dir, &entries)?;

        Ok(Some(NativeArtifactPaths {
            object_path,
            source_map_path,
            manifest_path,
            capabilities_path,
        }))
    }

    /// Load a persisted native artifact by name or hash.
    ///
    /// Lookup order:
    /// 1. If `name` is a 64-char hex string → exact hash match in index.
    /// 2. Strip `.o` suffix → match against `profile` in index (latest by `stored_at`).
    /// 3. If no profile match and the name carries no foreign extension, return the
    ///    latest entry overall.  Names with a foreign extension (e.g. `"dev.wasm"`)
    ///    suppress the fallback so they cannot resolve via the native index.
    ///
    /// Returns `Ok(None)` when no persisted artifact is found or when using a
    /// non-file-backed backend.
    pub fn load_native_artifact(
        &self,
        name: &str,
    ) -> Result<Option<PersistedNativeArtifact>, CliError> {
        let StoreHandle::File { ail_dir, .. } = self else {
            return Ok(None);
        };
        let entries = read_native_artifact_index(ail_dir).unwrap_or_default();
        if entries.is_empty() {
            return Ok(None);
        }

        let entry = if is_object_file_name(name) {
            // Exact hash lookup.
            entries.iter().find(|e| e.hash == name)
        } else {
            // Profile-name match (strip optional .o suffix), then fall back to latest.
            // Suppress the fallback when the name carries a foreign extension (e.g. ".wasm"):
            // those names belong to other artifact types and must not resolve via native fallback.
            let profile_guess = name.strip_suffix(".o").unwrap_or(name);
            let has_foreign_ext = name.contains('.') && !name.ends_with(".o");
            entries
                .iter()
                .filter(|e| e.profile == profile_guess)
                .max_by_key(|e| e.stored_at)
                .or_else(|| {
                    if has_foreign_ext {
                        None
                    } else {
                        entries.iter().max_by_key(|e| e.stored_at)
                    }
                })
        };

        let Some(entry) = entry else {
            return Ok(None);
        };

        let dir = native_dir(ail_dir);
        let object_path = dir.join(format!("{}.o", entry.hash));
        let source_map_path = dir.join(format!("{}.source_map.json", entry.hash));
        let manifest_path = dir.join(format!("{}.manifest.json", entry.hash));
        let capabilities_path = dir.join(format!("{}.capabilities.json", entry.hash));

        if !object_path.exists() {
            return Ok(None);
        }

        let object_bytes = std::fs::read(&object_path)?;
        let source_map_json = if source_map_path.exists() {
            std::fs::read(&source_map_path)?
        } else {
            b"{}".to_vec()
        };
        let artifact_manifest_json = if manifest_path.exists() {
            std::fs::read(&manifest_path)?
        } else {
            b"{}".to_vec()
        };
        let capabilities_manifest_json = if capabilities_path.exists() {
            std::fs::read(&capabilities_path)?
        } else {
            b"{\"entries\":[]}".to_vec()
        };

        Ok(Some(PersistedNativeArtifact {
            hash: entry.hash.clone(),
            profile: entry.profile.clone(),
            target: entry.target.clone(),
            object_bytes,
            source_map_json,
            artifact_manifest_json,
            capabilities_manifest_json,
            paths: NativeArtifactPaths {
                object_path,
                source_map_path,
                manifest_path,
                capabilities_path,
            },
        }))
    }

    /// Load a persisted WASM artifact by name or hash.
    ///
    /// Lookup order:
    /// 1. If `name` is a 64-char hex string → exact hash match in index.
    /// 2. Strip `.wasm` suffix → match against `profile` in index (latest by `stored_at`).
    /// 3. Return the latest entry overall.
    ///
    /// Returns `Ok(None)` when no persisted artifact is found or when using a
    /// non-file-backed backend.
    pub fn load_wasm_artifact(
        &self,
        name: &str,
    ) -> Result<Option<PersistedWasmArtifact>, CliError> {
        let StoreHandle::File { ail_dir, .. } = self else {
            return Ok(None);
        };
        let entries = read_wasm_artifact_index(ail_dir).unwrap_or_default();
        if entries.is_empty() {
            return Ok(None);
        }

        let entry = if is_object_file_name(name) {
            // Exact hash lookup.
            entries.iter().find(|e| e.hash == name)
        } else {
            // Profile-name match (strip optional .wasm suffix), then fall back to latest.
            // Suppress the fallback when the name carries a foreign extension (e.g. ".o"):
            // those names belong to other artifact types and must not resolve via WASM fallback.
            let profile_guess = name.strip_suffix(".wasm").unwrap_or(name);
            let has_foreign_ext = name.contains('.') && !name.ends_with(".wasm");
            entries
                .iter()
                .filter(|e| e.profile == profile_guess)
                .max_by_key(|e| e.stored_at)
                .or_else(|| {
                    if has_foreign_ext {
                        None
                    } else {
                        entries.iter().max_by_key(|e| e.stored_at)
                    }
                })
        };

        let Some(entry) = entry else {
            return Ok(None);
        };

        let dir = wasm_dir(ail_dir);
        let wasm_path = dir.join(format!("{}.wasm", entry.hash));
        let source_map_path = dir.join(format!("{}.source_map.json", entry.hash));
        let manifest_path = dir.join(format!("{}.manifest.json", entry.hash));
        let capabilities_path = dir.join(format!("{}.capabilities.json", entry.hash));
        let abi_descriptor_path = dir.join(format!("{}.abi.json", entry.hash));

        if !wasm_path.exists() {
            return Ok(None);
        }

        let wasm_bytes = std::fs::read(&wasm_path)?;
        let source_map_json = if source_map_path.exists() {
            std::fs::read(&source_map_path)?
        } else {
            b"{}".to_vec()
        };
        let artifact_manifest_json = if manifest_path.exists() {
            std::fs::read(&manifest_path)?
        } else {
            b"{}".to_vec()
        };
        let capabilities_manifest_json = if capabilities_path.exists() {
            std::fs::read(&capabilities_path)?
        } else {
            b"{\"entries\":[]}".to_vec()
        };
        let abi_descriptor_json = if abi_descriptor_path.exists() {
            Some(std::fs::read(&abi_descriptor_path)?)
        } else {
            None
        };
        let abi_descriptor_path = abi_descriptor_path.exists().then_some(abi_descriptor_path);

        Ok(Some(PersistedWasmArtifact {
            hash: entry.hash.clone(),
            profile: entry.profile.clone(),
            target: entry.target.clone(),
            wasm_bytes,
            source_map_json,
            artifact_manifest_json,
            capabilities_manifest_json,
            abi_descriptor_json,
            paths: WasmArtifactPaths {
                wasm_path,
                source_map_path,
                manifest_path,
                capabilities_path,
                abi_descriptor_path,
            },
        }))
    }
}

// ── WASM artifact index helpers ───────────────────────────────────────────

fn wasm_dir(ail_dir: &Path) -> PathBuf {
    ail_dir.join("wasm")
}

fn wasm_artifact_index_path(ail_dir: &Path) -> PathBuf {
    wasm_dir(ail_dir).join("artifact-index.json")
}

fn read_wasm_artifact_index(ail_dir: &Path) -> Result<Vec<WasmArtifactIndexEntry>, CliError> {
    let path = wasm_artifact_index_path(ail_dir);
    if !path.exists() {
        return Ok(vec![]);
    }
    let bytes = std::fs::read(&path)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| CliError::Domain(format!("wasm artifact index decode: {e}")))
}

fn write_wasm_artifact_index(
    ail_dir: &Path,
    entries: &[WasmArtifactIndexEntry],
) -> Result<(), CliError> {
    let bytes = serde_json::to_vec(entries)
        .map_err(|e| CliError::Domain(format!("wasm artifact index encode: {e}")))?;
    atomic_write(&wasm_artifact_index_path(ail_dir), &bytes)?;
    Ok(())
}

// ── Native artifact index helpers ─────────────────────────────────────────

fn native_dir(ail_dir: &Path) -> PathBuf {
    ail_dir.join("native")
}

fn native_artifact_index_path(ail_dir: &Path) -> PathBuf {
    native_dir(ail_dir).join("artifact-index.json")
}

fn read_native_artifact_index(ail_dir: &Path) -> Result<Vec<NativeArtifactIndexEntry>, CliError> {
    let path = native_artifact_index_path(ail_dir);
    if !path.exists() {
        return Ok(vec![]);
    }
    let bytes = std::fs::read(&path)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| CliError::Domain(format!("native artifact index decode: {e}")))
}

fn write_native_artifact_index(
    ail_dir: &Path,
    entries: &[NativeArtifactIndexEntry],
) -> Result<(), CliError> {
    let bytes = serde_json::to_vec(entries)
        .map_err(|e| CliError::Domain(format!("native artifact index encode: {e}")))?;
    atomic_write(&native_artifact_index_path(ail_dir), &bytes)?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{file_store, init_file_layout, memory_store};

    // ── T4: WASM artifact persistence ─────────────────────────────────────

    // Scenario: save_wasm_artifact returns None for memory backend (no-op).
    //   GIVEN a memory StoreHandle
    //   WHEN save_wasm_artifact is called
    //   THEN Ok(None) is returned
    #[test]
    fn save_wasm_artifact_memory_is_noop() {
        let store = memory_store();
        let result = store.save_wasm_artifact(
            &"a".repeat(64),
            "dev",
            "wasm",
            WasmArtifactBytes {
                wasm: b"fake-wasm",
                source_map_json: b"{}",
                artifact_manifest_json: b"{}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        );
        assert!(result.is_ok(), "save_wasm_artifact must not error");
        assert!(
            result.unwrap().is_none(),
            "memory store must return None (no-op)"
        );
    }

    // Scenario: save_wasm_artifact writes four sidecar files for file backend.
    //   GIVEN a file StoreHandle
    //   WHEN save_wasm_artifact is called
    //   THEN .ail/wasm/<hash>.wasm and the three sidecar files are on disk
    #[test]
    fn save_wasm_artifact_file_writes_sidecars() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir.clone());

        let fake_hash = "c".repeat(64);
        let result = store.save_wasm_artifact(
            &fake_hash,
            "dev",
            "wasm",
            WasmArtifactBytes {
                wasm: b"fake-wasm-bytes",
                source_map_json: b"{\"mappings\":[]}",
                artifact_manifest_json: b"{\"profile\":\"dev\"}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        );
        let paths = result
            .expect("save_wasm_artifact must succeed")
            .expect("file store must return Some(paths)");

        assert!(paths.wasm_path.exists(), "wasm file must exist");
        assert!(paths.source_map_path.exists(), "source_map file must exist");
        assert!(paths.manifest_path.exists(), "manifest file must exist");
        assert!(
            paths.capabilities_path.exists(),
            "capabilities file must exist"
        );

        let wasm_bytes = std::fs::read(&paths.wasm_path).expect("read wasm");
        assert_eq!(wasm_bytes, b"fake-wasm-bytes", "wasm bytes must match");

        // Index must also be written.
        let index_path = ail_dir.join("wasm").join("artifact-index.json");
        assert!(index_path.exists(), "artifact-index.json must be written");
        let index_bytes = std::fs::read(&index_path).expect("read index");
        let index: Vec<WasmArtifactIndexEntry> =
            serde_json::from_slice(&index_bytes).expect("parse index");
        assert_eq!(index.len(), 1, "index must have one entry");
        assert_eq!(index[0].hash, fake_hash, "index hash must match");
        assert_eq!(index[0].profile, "dev");
        assert_eq!(index[0].target, "wasm");
    }

    // Scenario: save_wasm_artifact_with_abi writes and reloads ABI descriptor sidecar.
    //   GIVEN a file StoreHandle
    //   WHEN a WASM artifact is saved with an ABI descriptor
    //   THEN .ail/wasm/<hash>.abi.json is on disk and load_wasm_artifact returns it
    #[test]
    fn wasm_artifact_save_load_roundtrip_with_abi_descriptor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir);

        let fake_hash = "b".repeat(64);
        let abi_descriptor = b"{\"abi_version\":1,\"exports\":{\"main\":{\"Scalar\":\"I64\"}}}";
        let paths = store
            .save_wasm_artifact_with_abi(
                &fake_hash,
                "dev",
                "wasm",
                WasmArtifactBytes {
                    wasm: b"wasm-with-abi",
                    source_map_json: b"{}",
                    artifact_manifest_json: b"{}",
                    capabilities_manifest_json: b"{\"entries\":[]}",
                },
                abi_descriptor,
            )
            .expect("save must succeed")
            .expect("file store must return Some(paths)");

        let abi_path = paths
            .abi_descriptor_path
            .as_ref()
            .expect("ABI descriptor path must be returned");
        assert!(abi_path.exists(), "ABI descriptor sidecar must exist");
        assert_eq!(
            std::fs::read(abi_path).expect("read ABI descriptor"),
            abi_descriptor
        );

        let loaded = store
            .load_wasm_artifact("dev.wasm")
            .expect("load must not error")
            .expect("artifact must be found");
        assert_eq!(
            loaded.abi_descriptor_json.as_deref(),
            Some(&abi_descriptor[..])
        );
        assert!(
            loaded
                .paths
                .abi_descriptor_path
                .as_ref()
                .is_some_and(|path| path.exists()),
            "loaded paths must include the ABI descriptor sidecar"
        );
    }

    // Scenario: load_wasm_artifact returns None for memory backend.
    //   GIVEN a memory StoreHandle
    //   WHEN load_wasm_artifact is called
    //   THEN Ok(None) is returned
    #[test]
    fn load_wasm_artifact_memory_returns_none() {
        let store = memory_store();
        let result = store.load_wasm_artifact("dev.wasm");
        assert!(result.is_ok(), "load_wasm_artifact must not error");
        assert!(result.unwrap().is_none(), "memory store must return None");
    }

    // Scenario: save + load roundtrip for file backend.
    //   GIVEN a file StoreHandle with a saved artifact
    //   WHEN load_wasm_artifact is called with a matching profile name
    //   THEN the loaded artifact bytes match the saved bytes
    #[test]
    fn wasm_artifact_save_load_roundtrip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir.clone());

        let fake_hash = "d".repeat(64);
        store
            .save_wasm_artifact(
                &fake_hash,
                "dev",
                "wasm",
                WasmArtifactBytes {
                    wasm: b"wasm-roundtrip",
                    source_map_json: b"{\"sm\":1}",
                    artifact_manifest_json: b"{\"mf\":1}",
                    capabilities_manifest_json: b"{\"entries\":[]}",
                },
            )
            .expect("save must succeed");

        // Load by profile name.
        let loaded = store
            .load_wasm_artifact("dev.wasm")
            .expect("load must not error")
            .expect("artifact must be found");

        assert_eq!(loaded.hash, fake_hash, "hash must match");
        assert_eq!(loaded.profile, "dev", "profile must match");
        assert_eq!(
            loaded.wasm_bytes, b"wasm-roundtrip",
            "wasm bytes must match"
        );
        assert_eq!(loaded.source_map_json, b"{\"sm\":1}");
        assert_eq!(loaded.artifact_manifest_json, b"{\"mf\":1}");
        assert_eq!(loaded.capabilities_manifest_json, b"{\"entries\":[]}");
    }

    // TRIANGULATE: load_wasm_artifact returns None when no artifact persisted.
    //   GIVEN a file StoreHandle with no saved artifact
    //   WHEN load_wasm_artifact is called
    //   THEN Ok(None) is returned
    #[test]
    fn load_wasm_artifact_file_returns_none_when_empty() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir);

        let result = store.load_wasm_artifact("program.wasm");
        assert!(result.is_ok(), "must not error");
        assert!(
            result.unwrap().is_none(),
            "no persisted artifact must return None"
        );
    }

    // TRIANGULATE: exact hash lookup in load_wasm_artifact.
    //   GIVEN a file store with a saved artifact
    //   WHEN load_wasm_artifact is called with the 64-char hash as name
    //   THEN the artifact is returned
    #[test]
    fn load_wasm_artifact_by_exact_hash() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir);

        let fake_hash = "e".repeat(64);
        store
            .save_wasm_artifact(
                &fake_hash,
                "prod",
                "wasm",
                WasmArtifactBytes {
                    wasm: b"prod-wasm",
                    source_map_json: b"{}",
                    artifact_manifest_json: b"{}",
                    capabilities_manifest_json: b"{\"entries\":[]}",
                },
            )
            .expect("save must succeed");

        let loaded = store
            .load_wasm_artifact(&fake_hash)
            .expect("load must not error")
            .expect("artifact must be found by hash");

        assert_eq!(loaded.hash, fake_hash);
        assert_eq!(loaded.profile, "prod");
    }

    // ── T5: Native artifact persistence ──────────────────────────────────

    // Scenario: save_native_artifact returns None for memory backend (no-op).
    //   GIVEN a memory StoreHandle
    //   WHEN save_native_artifact is called
    //   THEN Ok(None) is returned
    #[test]
    fn save_native_artifact_memory_is_noop() {
        let store = memory_store();
        let result = store.save_native_artifact(
            &"a".repeat(64),
            "dev",
            "native",
            NativeArtifactBytes {
                object: b"fake-object",
                source_map_json: b"{}",
                artifact_manifest_json: b"{}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        );
        assert!(result.is_ok(), "save_native_artifact must not error");
        assert!(
            result.unwrap().is_none(),
            "memory store must return None (no-op)"
        );
    }

    // Scenario: save_native_artifact writes four sidecar files for file backend.
    //   GIVEN a file StoreHandle
    //   WHEN save_native_artifact is called
    //   THEN .ail/native/<hash>.o and the three sidecar files are on disk
    #[test]
    fn save_native_artifact_file_writes_sidecars() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir.clone());

        let fake_hash = "c".repeat(64);
        let result = store.save_native_artifact(
            &fake_hash,
            "dev",
            "native",
            NativeArtifactBytes {
                object: b"fake-native-bytes",
                source_map_json: b"{\"mappings\":[]}",
                artifact_manifest_json: b"{\"profile\":\"dev\"}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        );
        let paths = result
            .expect("save_native_artifact must succeed")
            .expect("file store must return Some(paths)");

        assert!(paths.object_path.exists(), "object file must exist");
        assert!(paths.source_map_path.exists(), "source_map file must exist");
        assert!(paths.manifest_path.exists(), "manifest file must exist");
        assert!(
            paths.capabilities_path.exists(),
            "capabilities file must exist"
        );

        let object_bytes = std::fs::read(&paths.object_path).expect("read object");
        assert_eq!(
            object_bytes, b"fake-native-bytes",
            "object bytes must match"
        );

        // Index must also be written.
        let index_path = ail_dir.join("native").join("artifact-index.json");
        assert!(index_path.exists(), "artifact-index.json must be written");
        let index_bytes = std::fs::read(&index_path).expect("read index");
        let index: Vec<NativeArtifactIndexEntry> =
            serde_json::from_slice(&index_bytes).expect("parse index");
        assert_eq!(index.len(), 1, "index must have one entry");
        assert_eq!(index[0].hash, fake_hash, "index hash must match");
        assert_eq!(index[0].profile, "dev");
        assert_eq!(index[0].target, "native");
    }

    // Scenario: load_native_artifact returns None for memory backend.
    //   GIVEN a memory StoreHandle
    //   WHEN load_native_artifact is called
    //   THEN Ok(None) is returned
    #[test]
    fn load_native_artifact_memory_returns_none() {
        let store = memory_store();
        let result = store.load_native_artifact("dev.o");
        assert!(result.is_ok(), "load_native_artifact must not error");
        assert!(result.unwrap().is_none(), "memory store must return None");
    }

    // Scenario: save + load roundtrip for file backend.
    //   GIVEN a file StoreHandle with a saved native artifact
    //   WHEN load_native_artifact is called with a matching profile name
    //   THEN the loaded artifact bytes match the saved bytes
    #[test]
    fn native_artifact_save_load_roundtrip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir.clone());

        let fake_hash = "d".repeat(64);
        store
            .save_native_artifact(
                &fake_hash,
                "dev",
                "native",
                NativeArtifactBytes {
                    object: b"native-roundtrip",
                    source_map_json: b"{\"sm\":1}",
                    artifact_manifest_json: b"{\"mf\":1}",
                    capabilities_manifest_json: b"{\"entries\":[]}",
                },
            )
            .expect("save must succeed");

        // Load by profile name (strip .o suffix).
        let loaded = store
            .load_native_artifact("dev.o")
            .expect("load must not error")
            .expect("artifact must be found");

        assert_eq!(loaded.hash, fake_hash, "hash must match");
        assert_eq!(loaded.profile, "dev", "profile must match");
        assert_eq!(
            loaded.object_bytes, b"native-roundtrip",
            "object bytes must match"
        );
        assert_eq!(loaded.source_map_json, b"{\"sm\":1}");
        assert_eq!(loaded.artifact_manifest_json, b"{\"mf\":1}");
        assert_eq!(loaded.capabilities_manifest_json, b"{\"entries\":[]}");
    }

    // TRIANGULATE: load_native_artifact returns None when no artifact persisted.
    //   GIVEN a file StoreHandle with no saved native artifact
    //   WHEN load_native_artifact is called
    //   THEN Ok(None) is returned
    #[test]
    fn load_native_artifact_file_returns_none_when_empty() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir);

        let result = store.load_native_artifact("program.o");
        assert!(result.is_ok(), "must not error");
        assert!(
            result.unwrap().is_none(),
            "no persisted artifact must return None"
        );
    }

    // TRIANGULATE: exact hash lookup in load_native_artifact.
    //   GIVEN a file store with a saved native artifact
    //   WHEN load_native_artifact is called with the 64-char hash as name
    //   THEN the artifact is returned
    #[test]
    fn load_native_artifact_by_exact_hash() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir);

        let fake_hash = "e".repeat(64);
        store
            .save_native_artifact(
                &fake_hash,
                "prod",
                "native",
                NativeArtifactBytes {
                    object: b"prod-native",
                    source_map_json: b"{}",
                    artifact_manifest_json: b"{}",
                    capabilities_manifest_json: b"{\"entries\":[]}",
                },
            )
            .expect("save must succeed");

        let loaded = store
            .load_native_artifact(&fake_hash)
            .expect("load must not error")
            .expect("artifact must be found by hash");

        assert_eq!(loaded.hash, fake_hash);
        assert_eq!(loaded.profile, "prod");
    }

    // Regression: load_native_artifact must NOT fall back to latest for foreign-extension names.
    //   GIVEN a file store with one saved native artifact (profile "dev")
    //   WHEN load_native_artifact is called with "dev.wasm" (foreign extension)
    //   THEN Ok(None) is returned (fallback suppressed)
    //   AND  load_native_artifact("dev.o") still returns Some (own extension unaffected)
    #[test]
    fn load_native_artifact_suppresses_fallback_for_foreign_extension() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ail_dir = temp.path().join(".ail");
        init_file_layout(&ail_dir).expect("init layout");
        let store = file_store(ail_dir);

        let fake_hash = "f".repeat(64);
        store
            .save_native_artifact(
                &fake_hash,
                "dev",
                "native",
                NativeArtifactBytes {
                    object: b"native-regression",
                    source_map_json: b"{}",
                    artifact_manifest_json: b"{}",
                    capabilities_manifest_json: b"{\"entries\":[]}",
                },
            )
            .expect("save must succeed");

        // Foreign extension: must NOT fall back to the persisted native artifact.
        let result = store
            .load_native_artifact("dev.wasm")
            .expect("load must not error");
        assert!(
            result.is_none(),
            "load_native_artifact must return None for a .wasm-suffixed name (foreign extension); \
             fallback-to-latest must be suppressed"
        );

        // Own extension: must still resolve correctly.
        let result = store
            .load_native_artifact("dev.o")
            .expect("load must not error");
        assert!(
            result.is_some(),
            "load_native_artifact must still return Some for 'dev.o' (own extension)"
        );
    }
}
