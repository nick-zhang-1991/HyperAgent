#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz prompt sanitization / parsing
    let _ = hyperagent::llm::sanitize_prompt(std::str::from_utf8(data).unwrap_or(""));
});
