#![no_main]
use libfuzzer_sys::fuzz_target;
use bv_core::loader::{parse_issues_with_options, ParseOptions};

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let mut rdr = text.as_bytes();
        let _ = parse_issues_with_options(&mut rdr, &ParseOptions::default(), |_| {});
    }
});
