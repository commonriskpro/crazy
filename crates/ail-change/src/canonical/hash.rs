use super::*;

// ── helpers ───────────────────────────────────────────────────────────────

/// Compute blake3 hash of `(op CBOR encoding | phase ordinal | index)`.
///
/// The index ensures two identical ops at different positions produce
/// distinct hashes, providing per-block uniqueness.
pub(super) fn compute_block_hash(op: &ChangeSetOp, idx: usize) -> BlockHash {
    let mut op_bytes: Vec<u8> = Vec::new();
    ciborium::into_writer(op, &mut op_bytes).expect("ChangeSetOp serialization must not fail");

    let mut hasher = blake3::Hasher::new();
    hasher.update(&op_bytes);
    hasher.update(&phase_order(op).to_le_bytes());
    hasher.update(&(idx as u64).to_le_bytes());

    BlockHash(*hasher.finalize().as_bytes())
}
