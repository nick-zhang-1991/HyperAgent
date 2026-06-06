#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz JSON deserialization used in config / API payloads
    let _: Result<serde_json::Value, _> = serde_json::from_slice(data);
});
