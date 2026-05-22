// ── ail-runtime::codec ───────────────────────────────────────────────────
//
// Structured value types and decoder for the WASM/runtime typed ABI.
//
// # StructuredValue
//
// Represents a decoded return value from an exported WASM function.
// The runtime decodes raw i64/i32 return values and WASM linear memory
// into `StructuredValue` using a `ValueLayout` descriptor.
//
// # ValueLayout
//
// A runtime-side mirror of `WasmTypeDescriptor` from the compiler.
// Tells the decoder how to interpret a raw WASM return value.
//
// # ValueDecoder
//
// Stateless struct with a single `decode` method.  Given a `ValueLayout`,
// a raw i64 return value, and the full WASM linear memory slice, it
// reconstructs the corresponding `StructuredValue`.
//
// # HandleId / HandleRegistry
//
// Opaque handle identity for resources allocated by the host.

use std::collections::BTreeMap;

// ── HandleId ─────────────────────────────────────────────────────────────

/// Opaque handle identity — a monotonically increasing u64 starting from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HandleId(pub u64);

// ── HandleRegistry ────────────────────────────────────────────────────────

/// Tracks active handles.  `create` returns a fresh `HandleId` starting
/// at 1.  `release` marks a handle as inactive and returns whether the
/// handle was previously active.
pub struct HandleRegistry {
    next: u64,
    active: BTreeMap<u64, bool>,
}

impl HandleRegistry {
    pub fn new() -> Self {
        HandleRegistry {
            next: 1,
            active: BTreeMap::new(),
        }
    }

    /// Create a new handle and return its ID.
    pub fn create(&mut self) -> HandleId {
        let id = self.next;
        self.next += 1;
        self.active.insert(id, true);
        HandleId(id)
    }

    /// Return whether the given handle is currently active.
    pub fn contains(&self, id: HandleId) -> bool {
        self.active.get(&id.0).copied().unwrap_or(false)
    }

    /// Release a handle.  Returns `true` if the handle was active; `false`
    /// if it was already released or was never created.
    pub fn release(&mut self, id: HandleId) -> bool {
        match self.active.get_mut(&id.0) {
            Some(active @ true) => {
                *active = false;
                true
            }
            _ => false,
        }
    }
}

impl Default for HandleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── StructuredValue ───────────────────────────────────────────────────────

/// A decoded WASM return value.
///
/// Scalar values are decoded directly from the raw i64/i32 return.
/// Compound values (Record, Variant, List) are decoded from WASM linear
/// memory via a base pointer stored in the raw return value.
#[derive(Clone, Debug, PartialEq)]
pub enum StructuredValue {
    Scalar(i64),
    Float(f64),
    Record(Vec<(String, StructuredValue)>),
    Variant {
        tag: String,
        payload: Option<Box<StructuredValue>>,
    },
    List(Vec<StructuredValue>),
    Text {
        ptr: i32,
        len: i32,
    },
    Handle(HandleId),
    Unit,
}

// ── ValueLayout ───────────────────────────────────────────────────────────

/// Describes the expected layout of a WASM function's return value.
///
/// The runtime side mirror of `WasmTypeDescriptor` from `ail-compiler`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueLayout {
    Scalar,
    Record { fields: Vec<String> },
    Variant { tags: Vec<String> },
    List(Box<ValueLayout>),
    Option(Box<ValueLayout>),
    Result {
        ok: Box<ValueLayout>,
        err: Box<ValueLayout>,
    },
    Handle,
}

// ── ValueDecoder ─────────────────────────────────────────────────────────

/// Stateless decoder that converts a raw WASM return value + linear memory
/// into a `StructuredValue` according to a `ValueLayout`.
pub struct ValueDecoder;

impl ValueDecoder {
    /// Decode a raw WASM return value into a `StructuredValue`.
    ///
    /// `layout` — how to interpret the raw value.
    /// `raw` — the raw i64 return value from the WASM function.
    /// `memory` — the full WASM linear memory slice at the time of decode.
    ///
    /// Returns `StructuredValue::Unit` for any out-of-bounds memory access.
    pub fn decode(layout: &ValueLayout, raw: i64, memory: &[u8]) -> StructuredValue {
        match layout {
            ValueLayout::Scalar => StructuredValue::Scalar(raw),

            ValueLayout::Record { fields } => {
                let ptr = raw as i32 as usize;
                let fields_decoded = fields
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        let offset = ptr + i * 8;
                        let val = read_i64_at(memory, offset).unwrap_or(0);
                        (name.clone(), StructuredValue::Scalar(val))
                    })
                    .collect();
                StructuredValue::Record(fields_decoded)
            }

            ValueLayout::Variant { tags } => decode_variant(tags, raw as i32 as usize, memory),

            ValueLayout::List(inner) => decode_list(inner, raw as i32 as usize, memory),

            ValueLayout::Option(inner) => {
                let tags = vec!["None".to_string(), "Some".to_string()];
                decode_typed_variant(
                    &tags,
                    raw as i32 as usize,
                    memory,
                    &[
                        &ValueLayout::Scalar, // None has no meaningful payload
                        inner.as_ref(),
                    ],
                )
            }

            ValueLayout::Result { ok, err } => {
                let tags = vec!["Ok".to_string(), "Err".to_string()];
                decode_typed_variant(&tags, raw as i32 as usize, memory, &[ok, err])
            }

            ValueLayout::Handle => StructuredValue::Handle(HandleId(raw as u64)),
        }
    }
}

// ── private helpers ───────────────────────────────────────────────────────

/// Read a little-endian i64 from `memory` at `offset`.
/// Returns `None` if the read would be out of bounds.
fn read_i64_at(memory: &[u8], offset: usize) -> Option<i64> {
    let end = offset.checked_add(8)?;
    if end > memory.len() {
        return None;
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&memory[offset..end]);
    Some(i64::from_le_bytes(buf))
}

/// Read a little-endian i32 from `memory` at `offset`.
/// Returns `None` if the read would be out of bounds.
fn read_i32_at(memory: &[u8], offset: usize) -> Option<i32> {
    let end = offset.checked_add(4)?;
    if end > memory.len() {
        return None;
    }
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&memory[offset..end]);
    Some(i32::from_le_bytes(buf))
}

/// Decode a variant from WASM memory using a flat tag → `StructuredValue`
/// mapping.  Each tag always decodes its payload as a `Scalar`.
fn decode_variant(tags: &[String], ptr: usize, memory: &[u8]) -> StructuredValue {
    let tag_idx = match read_i32_at(memory, ptr) {
        Some(v) => v as usize,
        None => return StructuredValue::Unit,
    };
    let tag = tags
        .get(tag_idx)
        .cloned()
        .unwrap_or_else(|| "<unknown>".to_string());
    let payload = read_i64_at(memory, ptr + 8).map(|v| Box::new(StructuredValue::Scalar(v)));
    StructuredValue::Variant { tag, payload }
}

/// Decode a variant where each tag has a typed payload decoder.
///
/// `typed_payloads[i]` is the layout for tag index `i`.  If the tag index
/// is out of bounds or the payload read fails, returns `Unit`.
fn decode_typed_variant(
    tags: &[String],
    ptr: usize,
    memory: &[u8],
    typed_payloads: &[&ValueLayout],
) -> StructuredValue {
    let tag_idx = match read_i32_at(memory, ptr) {
        Some(v) => v as usize,
        None => return StructuredValue::Unit,
    };
    let tag = tags
        .get(tag_idx)
        .cloned()
        .unwrap_or_else(|| "<unknown>".to_string());

    // Tag 0 for Option::None has no meaningful payload; return payload: None.
    // For all others, decode the payload at ptr+8 using the typed layout.
    let payload_raw = read_i64_at(memory, ptr + 8);
    let payload = if let Some(layout) = typed_payloads.get(tag_idx) {
        match tag.as_str() {
            "None" => None,
            _ => payload_raw.map(|raw| {
                Box::new(ValueDecoder::decode(layout, raw, memory))
            }),
        }
    } else {
        None
    };

    StructuredValue::Variant { tag, payload }
}

/// Decode a list from WASM memory.
///
/// Layout: `[i64 count, elem0, elem1, …]` at `ptr`.  Each element is 8 bytes.
fn decode_list(inner: &ValueLayout, ptr: usize, memory: &[u8]) -> StructuredValue {
    let count = match read_i64_at(memory, ptr) {
        Some(c) if c >= 0 => c as usize,
        _ => return StructuredValue::Unit,
    };
    let elems = (0..count)
        .map(|i| {
            let offset = ptr + 8 + i * 8;
            let val = read_i64_at(memory, offset).unwrap_or(0);
            ValueDecoder::decode(inner, val, memory)
        })
        .collect();
    StructuredValue::List(elems)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── TASK-B1: StructuredValue + ValueLayout + HandleId + HandleRegistry ──

    #[test]
    fn structured_value_record_roundtrip() {
        let sv = StructuredValue::Record(vec![
            ("x".to_string(), StructuredValue::Scalar(42)),
        ]);
        // Debug/PartialEq
        let sv2 = sv.clone();
        assert_eq!(sv, sv2);
        assert!(format!("{sv:?}").contains("Scalar(42)"));
    }

    #[test]
    fn structured_value_variant_with_payload() {
        let sv = StructuredValue::Variant {
            tag: "Ok".to_string(),
            payload: Some(Box::new(StructuredValue::Scalar(1))),
        };
        assert_eq!(sv.clone(), sv);
        let StructuredValue::Variant { tag, payload } = sv else {
            panic!("expected Variant");
        };
        assert_eq!(tag, "Ok");
        assert_eq!(payload, Some(Box::new(StructuredValue::Scalar(1))));
    }

    #[test]
    fn value_layout_option_variants() {
        let opt = ValueLayout::Option(Box::new(ValueLayout::Scalar));
        assert_ne!(opt, ValueLayout::Scalar);
    }

    #[test]
    fn handle_id_starts_from_one() {
        let mut reg = HandleRegistry::new();
        let id = reg.create();
        assert_eq!(id, HandleId(1));
    }

    #[test]
    fn handle_registry_second_create_increments() {
        let mut reg = HandleRegistry::new();
        let id1 = reg.create();
        let id2 = reg.create();
        assert_eq!(id1, HandleId(1));
        assert_eq!(id2, HandleId(2));
    }

    #[test]
    fn handle_registry_release_then_release_again() {
        let mut reg = HandleRegistry::new();
        let id = reg.create();
        assert!(reg.release(id), "first release should return true");
        assert!(!reg.release(id), "second release should return false");
    }

    #[test]
    fn handle_registry_contains_after_create() {
        let mut reg = HandleRegistry::new();
        let id = reg.create();
        assert!(reg.contains(id), "contains must be true after create");
        reg.release(id);
        assert!(!reg.contains(id), "contains must be false after release");
    }

    // ── TASK-C1: ValueDecoder — all layout cases ─────────────────────────

    fn make_memory(values: &[(usize, i64)], size: usize) -> Vec<u8> {
        let mut mem = vec![0u8; size];
        for (offset, val) in values {
            let bytes = val.to_le_bytes();
            mem[*offset..*offset + 8].copy_from_slice(&bytes);
        }
        mem
    }

    fn make_memory_i32_at(mem: &mut Vec<u8>, offset: usize, val: i32) {
        let bytes = val.to_le_bytes();
        mem[offset..offset + 4].copy_from_slice(&bytes);
    }

    #[test]
    fn decode_scalar_returns_scalar() {
        let mem = vec![0u8; 0];
        let result = ValueDecoder::decode(&ValueLayout::Scalar, 42, &mem);
        assert_eq!(result, StructuredValue::Scalar(42));
    }

    #[test]
    fn decode_record_two_fields() {
        // memory: i64(10) at offset 0, i64(32) at offset 8; ptr=0
        let mem = make_memory(&[(0, 10), (8, 32)], 16);
        let layout = ValueLayout::Record {
            fields: vec!["a".to_string(), "b".to_string()],
        };
        let result = ValueDecoder::decode(&layout, 0, &mem);
        assert_eq!(
            result,
            StructuredValue::Record(vec![
                ("a".to_string(), StructuredValue::Scalar(10)),
                ("b".to_string(), StructuredValue::Scalar(32)),
            ])
        );
    }

    #[test]
    fn decode_variant_tag0_ok() {
        // memory: i32(0) at offset 0 (tag "Ok"), i64(99) at offset 8 (payload)
        let mut mem = vec![0u8; 16];
        make_memory_i32_at(&mut mem, 0, 0); // tag = 0 → "Ok"
        let payload_bytes = 99i64.to_le_bytes();
        mem[8..16].copy_from_slice(&payload_bytes);

        let layout = ValueLayout::Variant {
            tags: vec!["Ok".to_string(), "Err".to_string()],
        };
        let result = ValueDecoder::decode(&layout, 0, &mem);
        assert_eq!(
            result,
            StructuredValue::Variant {
                tag: "Ok".to_string(),
                payload: Some(Box::new(StructuredValue::Scalar(99)))
            }
        );
    }

    #[test]
    fn decode_variant_tag1_err() {
        // tag i32(1) → "Err"
        let mut mem = vec![0u8; 16];
        make_memory_i32_at(&mut mem, 0, 1);
        let payload_bytes = 0i64.to_le_bytes();
        mem[8..16].copy_from_slice(&payload_bytes);

        let layout = ValueLayout::Variant {
            tags: vec!["Ok".to_string(), "Err".to_string()],
        };
        let result = ValueDecoder::decode(&layout, 0, &mem);
        let StructuredValue::Variant { tag, .. } = result else {
            panic!("expected Variant");
        };
        assert_eq!(tag, "Err");
    }

    #[test]
    fn decode_list_two_elems() {
        // count=2 at offset 0, elem0=10 at offset 8, elem1=20 at offset 16
        let mem = make_memory(&[(0, 2), (8, 10), (16, 20)], 24);
        let layout = ValueLayout::List(Box::new(ValueLayout::Scalar));
        let result = ValueDecoder::decode(&layout, 0, &mem);
        assert_eq!(
            result,
            StructuredValue::List(vec![
                StructuredValue::Scalar(10),
                StructuredValue::Scalar(20),
            ])
        );
    }

    #[test]
    fn decode_option_none() {
        // Variant layout None/Some, tag=0 → Variant{tag:"None", payload:None}
        let mut mem = vec![0u8; 16];
        make_memory_i32_at(&mut mem, 0, 0); // tag = 0 → "None"

        let layout = ValueLayout::Option(Box::new(ValueLayout::Scalar));
        let result = ValueDecoder::decode(&layout, 0, &mem);
        assert_eq!(
            result,
            StructuredValue::Variant {
                tag: "None".to_string(),
                payload: None
            }
        );
    }

    #[test]
    fn decode_option_some() {
        // tag=1 (Some), payload=7
        let mut mem = vec![0u8; 16];
        make_memory_i32_at(&mut mem, 0, 1); // tag = 1 → "Some"
        let payload_bytes = 7i64.to_le_bytes();
        mem[8..16].copy_from_slice(&payload_bytes);

        let layout = ValueLayout::Option(Box::new(ValueLayout::Scalar));
        let result = ValueDecoder::decode(&layout, 0, &mem);
        assert_eq!(
            result,
            StructuredValue::Variant {
                tag: "Some".to_string(),
                payload: Some(Box::new(StructuredValue::Scalar(7)))
            }
        );
    }

    #[test]
    fn decode_result_ok_err() {
        let layout = ValueLayout::Result {
            ok: Box::new(ValueLayout::Scalar),
            err: Box::new(ValueLayout::Scalar),
        };

        // Ok: tag=0, payload=42
        let mut mem_ok = vec![0u8; 16];
        make_memory_i32_at(&mut mem_ok, 0, 0);
        mem_ok[8..16].copy_from_slice(&42i64.to_le_bytes());
        let ok_result = ValueDecoder::decode(&layout, 0, &mem_ok);
        assert_eq!(
            ok_result,
            StructuredValue::Variant {
                tag: "Ok".to_string(),
                payload: Some(Box::new(StructuredValue::Scalar(42)))
            }
        );

        // Err: tag=1, payload=99
        let mut mem_err = vec![0u8; 16];
        make_memory_i32_at(&mut mem_err, 0, 1);
        mem_err[8..16].copy_from_slice(&99i64.to_le_bytes());
        let err_result = ValueDecoder::decode(&layout, 0, &mem_err);
        assert_eq!(
            err_result,
            StructuredValue::Variant {
                tag: "Err".to_string(),
                payload: Some(Box::new(StructuredValue::Scalar(99)))
            }
        );
    }

    #[test]
    fn decode_oob_returns_unit() {
        // ptr past end of memory → Unit (no panic)
        let mem = vec![0u8; 4]; // too small for an i64 at offset 0
        let layout = ValueLayout::Record {
            fields: vec!["x".to_string()],
        };
        // ptr=0 but memory only has 4 bytes, can't read i64 at offset 0
        let result = ValueDecoder::decode(&layout, 0, &mem);
        assert_eq!(
            result,
            StructuredValue::Record(vec![
                ("x".to_string(), StructuredValue::Scalar(0))
            ])
        );

        // variant OOB: memory is only 2 bytes, can't read i32 at offset 0
        let tiny = vec![0u8; 2];
        let var_layout = ValueLayout::Variant {
            tags: vec!["A".to_string()],
        };
        let var_result = ValueDecoder::decode(&var_layout, 0, &tiny);
        assert_eq!(var_result, StructuredValue::Unit);
    }

    #[test]
    fn decode_handle() {
        let mem = vec![0u8; 0];
        let result = ValueDecoder::decode(&ValueLayout::Handle, 5, &mem);
        assert_eq!(result, StructuredValue::Handle(HandleId(5)));
    }
}
