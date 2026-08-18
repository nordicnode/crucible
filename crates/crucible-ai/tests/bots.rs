//! M2 acceptance: economy reaches scale by minute 5, rush beats turtle, and
//! the hard bot beats the medium bot across a deterministic seed set.

use crucible_ai::{easy, hard, medium, run_match, Bot, MatchOutcome};
use crucible_sim::{GameConfig, Map, Player};

fn config() -> GameConfig {
    GameConfig::default()
}

#[test]
fn harvester_economy_scales_by_minute_5() {
    let mut g = crucible_sim::Game::new(Map::generate(42), config());
    let mut bot = easy();
    let ore_before: i32 = g.map.ore.iter().sum();

    // Drive P0 with the easy bot for 5 minutes of game time.
    while g.tick < 3_000 {
        if g.is_command_tick() {
            let cmds = bot.decide(&g, Player::P0);
            if !cmds.is_empty() {
                g.apply_commands(Player::P0, &cmds);
            }
        }
        g.step();
    }

    let harvesters = g
        .units
        .iter()
        .filter(|u| u.owner == Player::P0 && u.utype == crucible_sim::UnitType::Harvester)
        .count();
    assert!(
        harvesters >= 6,
        "economy did not build 6 harvesters (got {harvesters})"
    );

    // Harvesting must have actually depleted the ore fields (not just trickle).
    let ore_mined = ore_before - g.map.ore.iter().sum::<i32>();
    assert!(
        ore_mined >= 500,
        "harvesters mined too little ore by minute 5 (mined {ore_mined})"
    );
}

/// Medium (rush waves) vs easy (turtle) across a seed set.
#[test]
fn rush_beats_turtle() {
    let seeds: Vec<u64> = (0..10).map(|i| 1000 + i).collect();
    let mut wins = 0;
    let mut total = 0;
    for seed in seeds {
        let mut attacker = medium();
        let mut turtle = easy();
        let o = run_match(seed, &config(), &mut attacker, &mut turtle);
        total += 1;
        if o.won_by(Player::P0) {
            wins += 1;
        }
    }
    println!("rush vs turtle: {wins}/{total}");
    assert!(
        wins as f64 / total as f64 >= 0.8,
        "rush did not beat turtle decisively ({wins}/{total})"
    );
}

/// Hard (expand-and-push) vs medium (waves) across a seed set.
#[test]
fn hard_beats_medium() {
    let seeds: Vec<u64> = (0..10).map(|i| 2000 + i).collect();
    let mut wins = 0;
    let mut total = 0;
    for seed in seeds {
        let mut a = hard();
        let mut b = medium();
        let o = run_match(seed, &config(), &mut a, &mut b);
        total += 1;
        if o.won_by(Player::P0) {
            wins += 1;
        }
    }
    println!("hard vs medium: {wins}/{total}");
    assert!(
        wins as f64 / total as f64 >= 0.8,
        "hard bot did not beat medium decisively ({wins}/{total})"
    );
}

// Sanity: a single match always terminates and reports a winner or timeout.
#[test]
fn match_terminates() {
    let mut a = hard();
    let mut b = medium();
    let o: MatchOutcome = run_match(7777, &config(), &mut a, &mut b);
    assert!(o.duration_ticks > 0);
    assert!(o.winner.is_some() || o.reason.is_some());
}
