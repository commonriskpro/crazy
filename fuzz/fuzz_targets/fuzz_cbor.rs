#![no_main]

use ail_storage::codec::{CborCodec, ContentCodec};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Feed arbitrary bytes into CborCodec::decode.
    // The codec must never panic — it must return Ok or Err.
    // Decoding as bytes (Vec<u8>) exercises the ciborium CBOR decoder path.
    let codec = CborCodec;
    let _: Result<Vec<u8>, _> = codec.decode(data);
});
