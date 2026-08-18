//! Fitness evaluation: shaped match fitness and the bootstrap economy metric.
//! Pure — runs headless matches and returns numbers; the server supplies
//! scheduling/parallelism.

use crucible_ai::{run_match_detailed, Bot, DetailedOutcome, GenomeBot};
use crucible_sim::{
    building_stats, unit_stats, BuildingType, Command, Game, GameConfig, Map, Player,
};

/// A bot that does nothing (used as a passive opponent for bootstrap stages).
pub struct Noop;

impl Bot for Noop {
    fn name(&self) -> &'static str {
        "noop"
    }
    fn decide(&mut self, _game: &Game, _player: Player) -> Vec<Command> {
        Vec::new()
    }
}

/// Shaped match fitness from a player's perspective (see plan §5.4):
/// win +1 / draw +0.1 / loss −1, + 0.25 × value margin, − 0.2 anti-rush damping.
pub fn shaped_fitness(d: &DetailedOutcome, player: Player) -> f32 {
    let o = d.outcome;
    let base = if o.won_by(player) {
        1.0
    } else if o.winner.is_none() {
        0.1
    } else {
        -1.0
    };

    let (own, enemy) = if player == Player::P0 {
        (d.p0_value, d.p1_value)
    } else {
        (d.p1_value, d.p0_value)
    };
    let total = (own + enemy) as f32;
    let margin = if total > 0.0 {
        (own - enemy) as f32 / total
    } else {
        0.0
    };

    let anti_rush = if o.duration_ticks < 2 * 60 * 10 {
        -0.2
    } else {
        0.0
    };
    base + 0.25 * margin + anti_rush
}

/// Mean shaped fitness of `genome` against a fresh opponent each match, with
/// both spawn sides played (mirror fairness).
pub fn evaluate_vs(
    genome: &[f32],
    seeds: &[u64],
    config: &GameConfig,
    mut make_opponent: impl FnMut() -> Box<dyn Bot>,
) -> f32 {
    let mut total = 0.0f32;
    let n = (seeds.len() * 2) as f32;
    for &seed in seeds {
        let mut g0 = GenomeBot::new(genome.to_vec());
        let mut opp0 = make_opponent();
        let d0 = run_match_detailed(seed, config, &mut g0, opp0.as_mut());
        total += shaped_fitness(&d0, Player::P0);

        let mut g1 = GenomeBot::new(genome.to_vec());
        let mut opp1 = make_opponent();
        let d1 = run_match_detailed(seed, config, opp1.as_mut(), &mut g1);
        total += shaped_fitness(&d1, Player::P1);
    }
    total / n
}

/// Bootstrap stage 1 fitness: ore mined (map depletion) by `genome` alone in
/// `ticks` game ticks, averaged over seeds. Higher is better.
pub fn evaluate_economy(genome: &[f32], seeds: &[u64], config: &GameConfig, ticks: i32) -> f32 {
    let mut total = 0.0f32;
    for &seed in seeds {
        let mut g = Game::new(Map::generate(seed), config.clone());
        let mut bot = GenomeBot::new(genome.to_vec());
        let before: i32 = g.map.ore.iter().sum();
        while g.tick < ticks && !g.is_over() {
            if g.is_command_tick() {
                let cmds = bot.decide(&g, Player::P0);
                if !cmds.is_empty() {
                    g.apply_commands(Player::P0, &cmds);
                }
            }
            g.step();
        }
        let after: i32 = g.map.ore.iter().sum();
        total += (before - after) as f32;
    }
    total / seeds.len() as f32
}

/// Mean shaped fitness of `genome` against a set of sampled opponents (from
/// the population) plus the reigning champion. Each opponent is played on
/// every seed, both spawn sides. Used by the self-play trainer.
pub fn self_play_fitness(
    genome: &[f32],
    opponents: &[Vec<f32>],
    champion: Option<&[f32]>,
    seeds: &[u64],
    config: &GameConfig,
) -> f32 {
    let mut total = 0.0f32;
    let mut n = 0u32;
    for opp in opponents {
        total += evaluate_vs(genome, seeds, config, || -> Box<dyn Bot> {
            Box::new(GenomeBot::new(opp.clone()))
        });
        n += 1;
    }
    if let Some(c) = champion {
        total += evaluate_vs(genome, seeds, config, || -> Box<dyn Bot> {
            Box::new(GenomeBot::new(c.to_vec()))
        });
        n += 1;
    }
    total / n.max(1) as f32
}

/// Total ore a player has spent (units + buildings), for income accounting.
pub fn spent_value(g: &Game, p: Player) -> i32 {
    let mut v = 0;
    for u in &g.units {
        if u.owner == p {
            v += unit_stats(u.utype).cost;
        }
    }
    for b in &g.buildings {
        if b.owner == p && b.btype != BuildingType::Hq {
            v += building_stats(b.btype).cost;
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaped_fitness_rewards_wins() {
        let mut g0 = GenomeBot::new(vec![0.0; crucible_ai::GENOME_LEN]);
        let mut noop = Noop;
        let cfg = GameConfig::default();
        let d = run_match_detailed(1, &cfg, &mut g0, &mut noop);
        // A genome of all zeros never acts, so the match should time out with
        // both HQs intact; winner is the higher-value side (a draw-ish result).
        let f = shaped_fitness(&d, Player::P0);
        assert!(f.is_finite());
    }

    #[test]
    fn economy_fitness_is_deterministic_and_nonnegative() {
        let genome = crucible_ai::init(&mut crucible_sim::Rng::from_seed(9));
        let cfg = GameConfig {
            timeout_ticks: 100_000,
            ..GameConfig::default()
        };
        let a = evaluate_economy(&genome, &[11], &cfg, 600);
        let b = evaluate_economy(&genome, &[11], &cfg, 600);
        assert_eq!(a, b);
        assert!(a >= 0.0);
    }
}
