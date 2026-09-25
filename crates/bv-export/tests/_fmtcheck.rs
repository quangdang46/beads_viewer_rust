//! Go's `encoding/json` float rendering, as the interactive graph export
//! depends on it.
//!
//! The embedded graph JSON carries nine float metrics per node. Go writes a
//! `float64` as the shortest decimal that round-trips, *without* a trailing
//! `.0` for an integral value; `serde_json` always writes one. Left alone,
//! every single node would render its critical-path score as `2.0` where Go
//! writes `2`, so `graph_interactive.rs` formats the numbers itself through
//! `go_json_float`.
//!
//! These cases are the ones where the two renderers are most likely to drift
//! again, and the last group is where they are known to differ for a reason
//! that is not a bug.

use bv_export::graph_interactive::go_json_float;

#[test]
fn integral_values_lose_the_decimal_point() {
    // Go's floatEncoder uses 'f' with precision -1, so 2.0 is "2".
    assert_eq!(go_json_float(2.0), "2");
    assert_eq!(go_json_float(0.0), "0");
    assert_eq!(go_json_float(-0.0), "-0");
    assert_eq!(go_json_float(1.0), "1");
    assert_eq!(go_json_float(-3.0), "-3");
}

#[test]
fn ordinary_values_round_trip_at_full_precision() {
    // Precision -1 means the shortest representation that parses back to the
    // same double, not a fixed number of digits.
    assert_eq!(go_json_float(0.2), "0.2");
    assert_eq!(go_json_float(1.0 / 3.0), "0.3333333333333333");
    assert_eq!(go_json_float(0.1 + 0.2), "0.30000000000000004");
    assert_eq!(go_json_float(-0.7), "-0.7");
}

#[test]
fn the_exponent_branch_matches_go() {
    // Go switches to 'e' below 1e-6 and at or above 1e21, keeps the '+' on a
    // positive exponent, and drops the padding zero from a negative one.
    assert_eq!(go_json_float(1.5e-9), "1.5e-9");
    assert_eq!(go_json_float(9.9e-7), "9.9e-7");
    assert_eq!(go_json_float(1e21), "1e+21");
    // The boundaries are exclusive: 1e-6 exactly and anything just under 1e21
    // stay in the fixed branch.
    assert_eq!(go_json_float(1e-6), "0.000001");
    assert_eq!(go_json_float(9.99e20), "999000000000000000000");
}

#[test]
fn zero_stays_in_the_fixed_branch() {
    // Go's condition is `abs != 0 && (...)`, so zero never reaches 'e' even
    // though it is below 1e-6.
    assert_eq!(go_json_float(0.0), "0");
    assert_eq!(go_json_float(-0.0), "-0");
}
