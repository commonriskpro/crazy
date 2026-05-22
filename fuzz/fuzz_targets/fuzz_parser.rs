#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Accept arbitrary bytes; treat as UTF-8 if valid, otherwise skip.
    // parse_changeset is a pure function — any input must not panic.
    if let Ok(src) = std::str::from_utf8(data) {
        let _ = ail_change::parser::parse_changeset(src);
    }
});
