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

/// Tracks active handles with reference counts.
///
/// Each handle starts with a reference count of 1 after `create`.
/// `clone_handle` increments the count.  `release` decrements the count and
/// returns `true` only when the count reaches zero (the handle is fully
/// released).  Handles with mode Linear or Affine are expected to be released
/// exactly once; Shared handles may be cloned and released multiple times.
pub struct HandleRegistry {
    next: u64,
    /// Maps handle id → reference count.  Count 0 means the handle was fully
    /// released; the entry is kept for idempotent `release` / `contains` calls.
    ref_counts: BTreeMap<u64, u32>,
}

impl HandleRegistry {
    pub fn new() -> Self {
        HandleRegistry {
            next: 1,
            ref_counts: BTreeMap::new(),
        }
    }

    /// Create a new handle with an initial reference count of 1.
    pub fn create(&mut self) -> HandleId {
        let id = self.next;
        self.next += 1;
        self.ref_counts.insert(id, 1);
        HandleId(id)
    }

    /// Increment the reference count for `id`.  Returns `true` if the handle
    /// was active; `false` if it was never created or already fully released.
    pub fn clone_handle(&mut self, id: HandleId) -> bool {
        match self.ref_counts.get_mut(&id.0) {
            Some(count) if *count > 0 => {
                *count += 1;
                true
            }
            _ => false,
        }
    }

    /// Return whether the given handle is currently active (ref count > 0).
    pub fn contains(&self, id: HandleId) -> bool {
        self.ref_counts.get(&id.0).copied().unwrap_or(0) > 0
    }

    /// Decrement the reference count for `id`.  Returns `true` when the count
    /// reaches zero (the handle is fully released).  Returns `false` if the
    /// handle was already fully released or was never created.
    pub fn release(&mut self, id: HandleId) -> bool {
        match self.ref_counts.get_mut(&id.0) {
            Some(count) if *count > 0 => {
                *count -= 1;
                *count == 0
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
/// Compound values (Record, Tuple, Variant, List) are decoded from WASM linear
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
    /// A raw byte buffer identified by pointer and length in WASM linear
    /// memory.  Unlike [`StructuredValue::Text`], no UTF-8 assumption is
    /// made — the bytes are opaque.
    ///
    /// Both `ptr` and `len` are decoded from the packed i64 return value via
    /// `ptr = (raw & 0xFFFF_FFFF) as i32` and `len = (raw >> 32) as i32`.
    Bytes {
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
    /// A UTF-8 text value packed as `(len as i64) << 32 | (ptr as i64)` in
    /// the raw i64 return slot.  Decoded to `StructuredValue::Text { ptr, len }`
    /// without reading WASM linear memory.
    Text,
    /// A raw byte buffer packed as `(len as i64) << 32 | (ptr as i64)` in
    /// the raw i64 return slot.  Decoded to `StructuredValue::Bytes { ptr, len }`
    /// without reading WASM linear memory.
    ///
    /// Unlike [`ValueLayout::Text`], no UTF-8 assumption is made — the bytes
    /// are opaque.
    Bytes,
    Record {
        fields: Vec<String>,
    },
    Variant {
        tags: Vec<String>,
    },
    Tuple(Vec<ValueLayout>),
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
    /// Returns `StructuredValue::Unit` for invalid pointers, unknown tags, or
    /// out-of-bounds memory access.
    pub fn decode(layout: &ValueLayout, raw: i64, memory: &[u8]) -> StructuredValue {
        match layout {
            ValueLayout::Scalar => StructuredValue::Scalar(raw),

            ValueLayout::Text => {
                // Packed encoding: upper 32 bits = len, lower 32 bits = ptr.
                let ptr = (raw & 0xFFFF_FFFF) as i32;
                let len = ((raw >> 32) & 0xFFFF_FFFF) as i32;
                StructuredValue::Text { ptr, len }
            }

            ValueLayout::Bytes => {
                // Same packed encoding as Text: upper 32 bits = len, lower 32
                // bits = ptr.  No UTF-8 check — the bytes are opaque.
                let ptr = (raw & 0xFFFF_FFFF) as i32;
                let len = ((raw >> 32) & 0xFFFF_FFFF) as i32;
                StructuredValue::Bytes { ptr, len }
            }

            ValueLayout::Record { fields } => decode_record(fields, raw, memory),

            ValueLayout::Tuple(elems) => decode_tuple(elems, raw, memory),

            ValueLayout::Variant { tags } => match ptr_from_raw(raw) {
                Some(ptr) => decode_variant(tags, ptr, memory),
                None => StructuredValue::Unit,
            },

            ValueLayout::List(inner) => match ptr_from_raw(raw) {
                Some(ptr) => decode_list(inner, ptr, memory),
                None => StructuredValue::Unit,
            },

            ValueLayout::Option(inner) => {
                let tags = vec!["None".to_string(), "Some".to_string()];
                match ptr_from_raw(raw) {
                    Some(ptr) => decode_typed_variant(
                        &tags,
                        ptr,
                        memory,
                        &[
                            &ValueLayout::Scalar, // None has no meaningful payload
                            inner.as_ref(),
                        ],
                    ),
                    None => StructuredValue::Unit,
                }
            }

            ValueLayout::Result { ok, err } => {
                let tags = vec!["Ok".to_string(), "Err".to_string()];
                match ptr_from_raw(raw) {
                    Some(ptr) => decode_typed_variant(&tags, ptr, memory, &[ok, err]),
                    None => StructuredValue::Unit,
                }
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

fn ptr_from_raw(raw: i64) -> Option<usize> {
    let ptr = i32::try_from(raw).ok()?;
    usize::try_from(ptr).ok()
}

fn decode_record(fields: &[String], raw: i64, memory: &[u8]) -> StructuredValue {
    let Some(ptr) = ptr_from_raw(raw) else {
        return StructuredValue::Unit;
    };
    let mut decoded = Vec::with_capacity(fields.len());
    for (i, name) in fields.iter().enumerate() {
        let Some(offset) = ptr.checked_add(i * 8) else {
            return StructuredValue::Unit;
        };
        let Some(val) = read_i64_at(memory, offset) else {
            return StructuredValue::Unit;
        };
        decoded.push((name.clone(), StructuredValue::Scalar(val)));
    }
    StructuredValue::Record(decoded)
}

fn decode_tuple(elems: &[ValueLayout], raw: i64, memory: &[u8]) -> StructuredValue {
    let Some(ptr) = ptr_from_raw(raw) else {
        return StructuredValue::Unit;
    };
    let mut decoded = Vec::with_capacity(elems.len());
    for (i, layout) in elems.iter().enumerate() {
        let Some(offset) = ptr.checked_add(i * 8) else {
            return StructuredValue::Unit;
        };
        let Some(val) = read_i64_at(memory, offset) else {
            return StructuredValue::Unit;
        };
        decoded.push(ValueDecoder::decode(layout, val, memory));
    }
    StructuredValue::List(decoded)
}

/// Decode a variant from WASM memory using a flat tag → `StructuredValue`
/// mapping.  Each tag always decodes its payload as a `Scalar`.
fn decode_variant(tags: &[String], ptr: usize, memory: &[u8]) -> StructuredValue {
    let tag_idx = match read_i32_at(memory, ptr) {
        Some(v) => v as usize,
        None => return StructuredValue::Unit,
    };
    let Some(tag) = tags.get(tag_idx).cloned() else {
        return StructuredValue::Unit;
    };
    let payload_offset = match ptr.checked_add(8) {
        Some(offset) => offset,
        None => return StructuredValue::Unit,
    };
    let Some(payload_raw) = read_i64_at(memory, payload_offset) else {
        return StructuredValue::Unit;
    };
    let payload = Some(Box::new(StructuredValue::Scalar(payload_raw)));
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
    let Some(tag) = tags.get(tag_idx).cloned() else {
        return StructuredValue::Unit;
    };

    // Tag 0 for Option::None has no meaningful payload; return payload: None.
    // For all others, decode the payload at ptr+8 using the typed layout.
    let payload = if let Some(layout) = typed_payloads.get(tag_idx) {
        match tag.as_str() {
            "None" => None,
            _ => {
                let payload_offset = match ptr.checked_add(8) {
                    Some(offset) => offset,
                    None => return StructuredValue::Unit,
                };
                let Some(payload_raw) = read_i64_at(memory, payload_offset) else {
                    return StructuredValue::Unit;
                };
                Some(Box::new(ValueDecoder::decode(layout, payload_raw, memory)))
            }
        }
    } else {
        return StructuredValue::Unit;
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
    let mut elems = Vec::with_capacity(count);
    for i in 0..count {
        let Some(offset) = ptr.checked_add(8 + i * 8) else {
            return StructuredValue::Unit;
        };
        let Some(val) = read_i64_at(memory, offset) else {
            return StructuredValue::Unit;
        };
        elems.push(ValueDecoder::decode(inner, val, memory));
    }
    StructuredValue::List(elems)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── TASK-B1: StructuredValue + ValueLayout + HandleId + HandleRegistry ──

    #[test]
    fn structured_value_record_roundtrip() {
        let sv = StructuredValue::Record(vec![("x".to_string(), StructuredValue::Scalar(42))]);
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

    fn make_memory_i32_at(mem: &mut [u8], offset: usize, val: i32) {
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
    fn decode_tuple_preserves_slot_order() {
        let mem = make_memory(&[(0, 11), (8, 22)], 16);
        let layout = ValueLayout::Tuple(vec![ValueLayout::Scalar, ValueLayout::Scalar]);
        let result = ValueDecoder::decode(&layout, 0, &mem);
        assert_eq!(
            result,
            StructuredValue::List(vec![
                StructuredValue::Scalar(11),
                StructuredValue::Scalar(22),
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
        assert_eq!(result, StructuredValue::Unit);

        let list_layout = ValueLayout::List(Box::new(ValueLayout::Scalar));
        let short_list = make_memory(&[(0, 2), (8, 10)], 16);
        assert_eq!(
            ValueDecoder::decode(&list_layout, 0, &short_list),
            StructuredValue::Unit
        );

        // variant OOB: memory is only 2 bytes, can't read i32 at offset 0
        let tiny = vec![0u8; 2];
        let var_layout = ValueLayout::Variant {
            tags: vec!["A".to_string()],
        };
        let var_result = ValueDecoder::decode(&var_layout, 0, &tiny);
        assert_eq!(var_result, StructuredValue::Unit);

        // variant payload OOB: tag is readable, but payload slot at ptr+8 is not.
        let mut tag_only = vec![0u8; 4];
        make_memory_i32_at(&mut tag_only, 0, 0);
        assert_eq!(
            ValueDecoder::decode(&var_layout, 0, &tag_only),
            StructuredValue::Unit
        );
    }

    #[test]
    fn decode_unknown_variant_tag_returns_unit() {
        let mut mem = vec![0u8; 16];
        make_memory_i32_at(&mut mem, 0, 2);
        mem[8..16].copy_from_slice(&99i64.to_le_bytes());
        let layout = ValueLayout::Variant {
            tags: vec!["Ok".to_string(), "Err".to_string()],
        };

        assert_eq!(
            ValueDecoder::decode(&layout, 0, &mem),
            StructuredValue::Unit
        );
    }

    #[test]
    fn decode_handle() {
        let mem = vec![0u8; 0];
        let result = ValueDecoder::decode(&ValueLayout::Handle, 5, &mem);
        assert_eq!(result, StructuredValue::Handle(HandleId(5)));
    }

    // ── WASM ABI surface: Bytes layout ────────────────────────────────────

    // ValueLayout::Bytes exists, implements Clone and PartialEq.
    #[test]
    fn value_layout_bytes_exists_and_is_not_scalar() {
        let layout = ValueLayout::Bytes;
        assert_ne!(layout, ValueLayout::Scalar);
        let cloned = layout.clone();
        assert_eq!(cloned, ValueLayout::Bytes);
    }

    // StructuredValue::Bytes exists, implements Clone and PartialEq.
    #[test]
    fn structured_value_bytes_exists_and_is_not_scalar() {
        let sv = StructuredValue::Bytes { ptr: 128, len: 32 };
        assert_ne!(sv, StructuredValue::Scalar(0));
        let cloned = sv.clone();
        assert_eq!(cloned, StructuredValue::Bytes { ptr: 128, len: 32 });
    }

    // ValueDecoder decodes Bytes from the packed i64 return slot.
    //
    // Encoding: ptr in the lower 32 bits, len in the upper 32 bits.
    // No memory read — both fields come from the raw i64 alone.
    #[test]
    fn decode_bytes_packed_encoding() {
        let mem = vec![0u8; 0]; // no memory needed
        let ptr: i32 = 256;
        let len: i32 = 48;
        // Pack: lower 32 = ptr, upper 32 = len
        let raw: i64 = ((len as i64) << 32) | (ptr as i64 & 0xFFFF_FFFF);
        let result = ValueDecoder::decode(&ValueLayout::Bytes, raw, &mem);
        assert_eq!(result, StructuredValue::Bytes { ptr: 256, len: 48 });
    }

    // Triangulate: Bytes with ptr=0,len=0 decodes to Bytes{0,0}.
    #[test]
    fn decode_bytes_zero_ptr_zero_len() {
        let mem = vec![0u8; 0];
        let result = ValueDecoder::decode(&ValueLayout::Bytes, 0, &mem);
        assert_eq!(result, StructuredValue::Bytes { ptr: 0, len: 0 });
    }

    // Bytes and Text decode identically from the packed i64, but produce
    // different StructuredValue variants (no UTF-8 assumption for Bytes).
    #[test]
    fn decode_bytes_and_text_produce_different_variants() {
        let mem = vec![0u8; 0];
        let raw: i64 = (10i64 << 32) | 64; // len=10, ptr=64
        let text_result = ValueDecoder::decode(&ValueLayout::Text, raw, &mem);
        let bytes_result = ValueDecoder::decode(&ValueLayout::Bytes, raw, &mem);
        // Both decode ptr=64 and len=10, but into different enum variants.
        assert_eq!(text_result, StructuredValue::Text { ptr: 64, len: 10 });
        assert_eq!(bytes_result, StructuredValue::Bytes { ptr: 64, len: 10 });
        assert_ne!(text_result, bytes_result);
    }
}
