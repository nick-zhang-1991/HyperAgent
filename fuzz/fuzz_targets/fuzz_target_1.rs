#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz diff parser — will panic on malformed input if not handled
    let _ = hyperagent::diff::parse_diff(std::str::from_utf8(data).unwrap_or(""));
});
