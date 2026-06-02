// ── ail-runtime::host_dispatch::memory ────────────────────────────────────

use wasmtime::Caller;

use crate::host_dispatch::state::HostState;
use crate::profile::CapabilityId;

pub(super) fn read_memory(
    caller: &mut Caller<'_, HostState>,
    ptr: i32,
    len: i32,
) -> Option<Vec<u8>> {
    if ptr < 0 || len < 0 {
        return None;
    }
    let memory = caller.get_export("memory")?.into_memory()?;
    let mut bytes = vec![0; len as usize];
    memory.read(caller, ptr as usize, &mut bytes).ok()?;
    Some(bytes)
}

fn decode_packed_text_arg(
    caller: &mut Caller<'_, HostState>,
    arg: &[u8],
    max_payload_bytes: Option<u64>,
) -> Option<Vec<u8>> {
    if arg.len() != 8 {
        return None;
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(arg);
    let packed = i64::from_le_bytes(raw);
    let ptr = (packed & 0xffff_ffff) as i32;
    let len = (packed >> 32) as i32;
    if len < 0 {
        return None;
    }
    if let Some(max_payload_bytes) = max_payload_bytes
        && len as u64 > max_payload_bytes
    {
        return None;
    }
    read_memory(caller, ptr, len)
}

pub(super) fn handler_payload(
    caller: &mut Caller<'_, HostState>,
    cap: &CapabilityId,
    operation: &str,
    args_bytes: &[u8],
    max_payload_bytes: Option<u64>,
) -> Option<Vec<u8>> {
    if (cap.as_str() == "log.write" && operation == "write")
        || (cap.as_str() == "file.read" && operation == "read")
    {
        let payload = decode_packed_text_arg(caller, args_bytes, max_payload_bytes)?;
        std::str::from_utf8(&payload).ok()?;
        Some(payload)
    } else {
        Some(args_bytes.to_vec())
    }
}
