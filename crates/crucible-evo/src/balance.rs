//! Balance harness: batch headless matchups → win-rate tables. Pure — runs
//! deterministic matches over a seed set and returns aggregate rates. Used for
//! the committed baseline tables and the CI regression check on sim changes.

use serde::{Deserialize, Serialize};

use crucible_ai::{run_match, Bot};
use crucible_sim::{
    unit_stats, Game, GameConfig, Map, Player, Pos, Stance, Unit, UnitOrder, UnitType,
};

/// Aggregate result of a matchup over a seed set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WinRate {
    pub matches: u32,
    pub a_wins: u32,
    pub b_wins: u32,
    pub draws: u32,
}

impl WinRate {
    pub fn a_rate(&self) -> f32 {
        self.a_wins as f32 / self.matches.max(1) as f32
    }
    pub fn b_rate(&self) -> f32 {
        self.b_wins as f32 / self.matches.max(1) as f32
    }
}

/// A one-sided unit composition (unit type → count). Costs of both sides in a
/// matchup should match for "equal cost".
pub type Composition = Vec<(UnitType, usize)>;

pub fn composition_cost(comp: &[(UnitType, usize)]) -> i32 {
    comp.iter()
        .map(|(t, n)| unit_stats(*t).cost * (*n as i32))
        .sum()
}

fn spawn_unit(g: &mut Game, p: Player, ut: UnitType, tile: (u8, u8)) {
    let stats = unit_stats(ut);
    let id = g.alloc_id();
    g.units.push(Unit {
        id,
        owner: p,
        utype: ut,
        pos: Pos::from_tile(tile.0, tile.1),
        hp: stats.hp,
        max_hp: stats.hp,
        stance: Stance::Aggressive,
        order: UnitOrder::Idle,
        carrying: 0,
        cooldown: 0,
        mining: 0,
        path: vec![],
        target: None,
        fleeing: false,
        harvest_tile: None,
        refinery: None,
    });
}

/// Spawn an army near `player`'s HQ and attack-move it toward the enemy HQ.
fn spawn_army(g: &mut Game, player: Player, comp: &[(UnitType, usize)]) {
    let hq = g.hq(player).unwrap().tile;
    let enemy = g.hq(player.enemy()).unwrap().tile;
    let dx = (enemy.0 as i32 - hq.0 as i32).signum();
    let dy = (enemy.1 as i32 - hq.1 as i32).signum();

    let mut i = 0usize;
    for (utype, n) in comp {
        for _ in 0..*n {
            let tx = (hq.0 as i32 + dx * 3 + (i % 5) as i32 - 2).clamp(0, 63) as u8;
            let ty = (hq.1 as i32 + dy * 3 + (i / 5) as i32 - 2).clamp(0, 63) as u8;
            spawn_unit(g, player, *utype, (tx, ty));
            i += 1;
        }
    }

    let units: Vec<u32> = g
        .units
        .iter()
        .filter(|u| u.owner == player)
        .map(|u| u.id)
        .collect();
    let _ = g.apply_commands(
        player,
        &[crucible_sim::Command::MoveGroup {
            player,
            units,
            waypoint: enemy,
            stance: Stance::Aggressive,
        }],
    );
}

/// Run one micro army-vs-army matchup on a procedural map and return the
/// winner (HQ destruction or timeout value).
pub fn micro_matchup(
    seed: u64,
    a: &[(UnitType, usize)],
    b: &[(UnitType, usize)],
    config: &GameConfig,
) -> Option<Player> {
    assert_eq!(
        composition_cost(a),
        composition_cost(b),
        "matchup must be equal cost"
    );
    let mut g = Game::new(Map::generate(seed), config.clone());
    spawn_army(&mut g, Player::P0, a);
    spawn_army(&mut g, Player::P1, b);
    let mut guard = 0;
    while !g.is_over() && guard < config.timeout_ticks + 1000 {
        g.step();
        guard += 1;
    }
    g.winner
}

/// Win rate of `a` (P0) vs `b` (P1) over a seed set.
pub fn micro_matchup_rate(
    seeds: &[u64],
    a: &[(UnitType, usize)],
    b: &[(UnitType, usize)],
    config: &GameConfig,
) -> WinRate {
    let mut rate = WinRate {
        matches: seeds.len() as u32,
        ..WinRate::default()
    };
    for &seed in seeds {
        match micro_matchup(seed, a, b, config) {
            Some(Player::P0) => rate.a_wins += 1,
            Some(Player::P1) => rate.b_wins += 1,
            None => rate.draws += 1,
        }
    }
    rate
}

/// Win rate of `make_a` (P0) vs `make_b` (P1) over a seed set.
pub fn bot_tier(
    seeds: &[u64],
    config: &GameConfig,
    mut make_a: impl FnMut() -> Box<dyn Bot>,
    mut make_b: impl FnMut() -> Box<dyn Bot>,
) -> WinRate {
    let mut rate = WinRate {
        matches: seeds.len() as u32,
        ..WinRate::default()
    };
    for &seed in seeds {
        let mut a = make_a();
        let mut b = make_b();
        match run_match(seed, config, a.as_mut(), b.as_mut()).winner {
            Some(Player::P0) => rate.a_wins += 1,
            Some(Player::P1) => rate.b_wins += 1,
            None => rate.draws += 1,
        }
    }
    rate
}

/// Median of a list of values (sorts a copy; stable for even-length inputs).
pub fn median(mut xs: Vec<i32>) -> i32 {
    xs.sort_unstable();
    xs[xs.len() / 2]
}

/// Match durations (in ticks) of a bot matchup over a seed set, in seed order.
pub fn bot_tier_lengths(
    seeds: &[u64],
    config: &GameConfig,
    mut make_a: impl FnMut() -> Box<dyn Bot>,
    mut make_b: impl FnMut() -> Box<dyn Bot>,
) -> Vec<i32> {
    seeds
        .iter()
        .map(|&seed| {
            let mut a = make_a();
            let mut b = make_b();
            run_match(seed, config, a.as_mut(), b.as_mut()).duration_ticks
        })
        .collect()
}

/// The three unit counter matchups (equal cost): tank>infantry, artillery>tank,
/// infantry>artillery. Returns (name, rate) with the counter as side `a`.
pub fn counter_matrix(seeds: &[u64], config: &GameConfig) -> Vec<(&'static str, WinRate)> {
    // 4 tanks (600) vs 12 infantry (600): tank splash counters infantry.
    let tank_vs_inf = micro_matchup_rate(
        seeds,
        &[(UnitType::Tank, 4)],
        &[(UnitType::Infantry, 12)],
        config,
    );
    // 3 artillery (600) vs 4 tanks (600): artillery outranges tanks.
    let art_vs_tank = micro_matchup_rate(
        seeds,
        &[(UnitType::Artillery, 3)],
        &[(UnitType::Tank, 4)],
        config,
    );
    // 12 infantry (600) vs 3 artillery (600): infantry closes inside min range.
    let inf_vs_art = micro_matchup_rate(
        seeds,
        &[(UnitType::Infantry, 12)],
        &[(UnitType::Artillery, 3)],
        config,
    );
    vec![
        ("tank>infantry", tank_vs_inf),
        ("artillery>tank", art_vs_tank),
        ("infantry>artillery", inf_vs_art),
    ]
}

/// The scripted bot tiers: easy vs medium, medium vs hard.
pub fn bot_tiers(seeds: &[u64], config: &GameConfig) -> Vec<(&'static str, WinRate)> {
    vec![
        (
            "medium>easy",
            bot_tier(
                seeds,
                config,
                || Box::new(crucible_ai::medium()),
                || Box::new(crucible_ai::easy()),
            ),
        ),
        (
            "hard>medium",
            bot_tier(
                seeds,
                config,
                || Box::new(crucible_ai::hard()),
                || Box::new(crucible_ai::medium()),
            ),
        ),
    ]
}

/// Full balance table as a serializable JSON value (committed as a baseline).
pub fn balance_table(seeds: &[u64], config: &GameConfig) -> serde_json::Value {
    let counters: Vec<serde_json::Value> = counter_matrix(seeds, config)
        .into_iter()
        .map(|(name, r)| serde_json::json!({ "matchup": name, "rate": r }))
        .collect();
    let tiers: Vec<serde_json::Value> = bot_tiers(seeds, config)
        .into_iter()
        .map(|(name, r)| serde_json::json!({ "matchup": name, "rate": r }))
        .collect();
    serde_json::json!({
        "seeds": seeds,
        "counters": counters,
        "bot_tiers": tiers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The acceptance band for M8: no unit may win its counter matchup more
    /// than 65% or less than 35% of the time at equal cost.
    fn in_band(r: WinRate) -> bool {
        (0.35..=0.65).contains(&r.a_rate()) && (0.35..=0.65).contains(&r.b_rate())
    }

    #[test]
    fn counter_matrix_is_deterministic_and_directional() {
        let cfg = GameConfig {
            timeout_ticks: 6000,
            ..GameConfig::default()
        };
        let seeds: Vec<u64> = (0..32).collect();
        let a = counter_matrix(&seeds, &cfg);
        let b = counter_matrix(&seeds, &cfg);
        assert_eq!(a, b);

        let rate_of = |name: &str| a.iter().find(|(n, _)| *n == name).map(|(_, r)| *r).unwrap();

        // The counter always wins, but softly (within the 35–65% band): tank
        // splash beats infantry, artillery outranges tank, infantry closes
        // inside artillery's min range.
        for (name, expected_winner_is_a) in [
            ("tank>infantry", true),
            ("artillery>tank", true),
            ("infantry>artillery", true),
        ] {
            let r = rate_of(name);
            assert!(in_band(r), "{name} left the 35–65% band: {r:?}");
            let wins = if expected_winner_is_a {
                r.a_rate()
            } else {
                r.b_rate()
            };
            assert!(wins > 0.5, "{name}: counter no longer wins ({r:?})");
        }
    }
}
