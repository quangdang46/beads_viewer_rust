#![no_main]
use libfuzzer_sys::fuzz_target;
use bv_core::model::Dependency;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _: Result<Dependency, _> = serde_json::from_str(text);
    }
});
