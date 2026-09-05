#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The redirect parser is exercised via discovery; here we test the
    // raw string handling that feeds it (trim, UTF-8 validation).
    if let Ok(s) = std::str::from_utf8(data) {
        let trimmed = s.trim();
        // Must not panic on any input
        let _ = trimmed.len();
        let _ = trimmed.is_empty();
    }
});
