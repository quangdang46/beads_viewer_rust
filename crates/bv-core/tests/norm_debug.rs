use bv_core::data_hash::normalize_rfc3339_nano;
#[test]
fn debug_norm() {
    println!(
        "norm: {:?}",
        normalize_rfc3339_nano("2026-08-22T08:09:34.064213Z")
    );
    panic!("dump");
}
