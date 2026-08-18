//! Cross-target determinism parity: the *same* golden scenario that runs
//! natively in `crucible-sim/tests/determinism.rs` runs here under
//! wasm (via wasm-bindgen-test/node) and must produce the *same* hashes.
//!
//! This closes the determinism loop — the golden constants are shared from
//! `crucible_sim::golden`, so native and wasm are compared against a single
//! source of truth, and a drift on either target fails CI.

use wasm_bindgen_test::*;

#[wasm_bindgen_test]
fn golden_hashes_match_native_constants() {
    let got = crucible_sim::golden::golden_hashes();
    assert_eq!(
        got,
        crucible_sim::golden::GOLDEN,
        "wasm golden hash drifted from native"
    );
}

#[wasm_bindgen_test]
fn same_seed_replays_byte_identical_under_wasm() {
    for tick in [1i32, 50, 200, 999, 5_000] {
        let ga = crucible_sim::golden::playout(777, tick);
        let gb = crucible_sim::golden::playout(777, tick);
        assert_eq!(
            crucible_sim::serialize::snapshot_bytes(&ga),
            crucible_sim::serialize::snapshot_bytes(&gb),
            "wasm divergence at tick {tick}"
        );
    }
}
