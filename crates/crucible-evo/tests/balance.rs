//! M8: the balance harness produces a committed baseline table. This test
//! regenerates it and asserts byte-stability, so any sim-affecting change that
//! moves a win rate is caught in CI.

use crucible_evo::{balance_table, bot_tier_lengths, median};
use crucible_sim::{GameConfig, TICKS_PER_SEC};

fn seeds() -> Vec<u64> {
    (0..32).collect()
}

fn config() -> GameConfig {
    GameConfig {
        timeout_ticks: 6000,
        ..GameConfig::default()
    }
}

#[test]
fn balance_table_is_deterministic() {
    let a = balance_table(&seeds(), &config());
    let b = balance_table(&seeds(), &config());
    assert_eq!(a, b);
}

/// Regenerate the baseline fixture (run manually: `cargo test -p crucible-evo --test balance -- --ignored --nocapture`).
#[test]
#[ignore]
fn dump_baseline() {
    println!(
        "{}",
        serde_json::to_string_pretty(&balance_table(&seeds(), &config())).unwrap()
    );
}

#[test]
fn balance_table_matches_baseline() {
    let table = balance_table(&seeds(), &config());
    let got = serde_json::to_string_pretty(&table).unwrap();

    let baseline = include_str!("fixtures/balance_baseline.json").trim_end();
    assert_eq!(
        got, baseline,
        "balance table drifted from the committed baseline; re-run and review \
         the numbers, then update the fixture"
    );
}

#[test]
fn match_length_p50_within_band() {
    // M8 target: match length p50 within 5–10 min (300–600s). The bot tiers
    // are the pacing anchor: rush-vs-turtle must not end instantly, and
    // hard-vs-medium must not stalemate to the timeout.
    let full = GameConfig::default(); // 15-minute timeout
    let seeds = seeds();

    let medium_easy = median(bot_tier_lengths(
        &seeds,
        &full,
        || Box::new(crucible_ai::medium()),
        || Box::new(crucible_ai::easy()),
    ));
    let hard_medium = median(bot_tier_lengths(
        &seeds,
        &full,
        || Box::new(crucible_ai::hard()),
        || Box::new(crucible_ai::medium()),
    ));

    let band = 5 * 60 * TICKS_PER_SEC..=10 * 60 * TICKS_PER_SEC;
    assert!(
        band.contains(&medium_easy),
        "medium-vs-easy p50 left the 5–10 min band: {}s",
        medium_easy / TICKS_PER_SEC
    );
    assert!(
        band.contains(&hard_medium),
        "hard-vs-medium p50 left the 5–10 min band: {}s",
        hard_medium / TICKS_PER_SEC
    );
}
