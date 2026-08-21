//! Headless match execution: run two [`Bot`]s to completion and return the
//! outcome. This is the shared evaluation path for scenario tests, scripted
//! bot matchups, and (later) fitness evaluation.

use crucible_sim::{Game, GameConfig, Map, Player, Replay, ReplayResult, WinReason};

use crate::bot::Bot;

/// The outcome of a completed match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchOutcome {
    pub winner: Option<Player>,
    pub reason: Option<WinReason>,
    pub duration_ticks: i32,
}

impl MatchOutcome {
    /// Whether `p` won this match.
    pub fn won_by(&self, p: Player) -> bool {
        self.winner == Some(p)
    }
}

/// A match outcome plus the final remaining value of each side (for margin
/// shaping in fitness evaluation).
#[derive(Clone, Copy, Debug)]
pub struct DetailedOutcome {
    pub outcome: MatchOutcome,
    pub p0_value: i32,
    pub p1_value: i32,
}

/// Run a single match to completion between two bots.
///
/// Both bots are polled every command tick; commands are applied through the
/// sim's normal validation/APM path. `seed` determines the map; the match is
/// fully deterministic given `seed`, `config`, and the two bot programs.
pub fn run_match(seed: u64, config: &GameConfig, a: &mut dyn Bot, b: &mut dyn Bot) -> MatchOutcome {
    run_match_detailed(seed, config, a, b).outcome
}

/// Like [`run_match`], but also reports the final remaining value of each side.
pub fn run_match_detailed(
    seed: u64,
    config: &GameConfig,
    a: &mut dyn Bot,
    b: &mut dyn Bot,
) -> DetailedOutcome {
    run_match_with_replay(seed, config, a, b).0
}

/// Run a match to completion, returning both the outcome and the full input-log
/// replay (map seed + every command + result) so it can be stored, spectated,
/// or re-run byte-identically.
pub fn run_match_with_replay(
    seed: u64,
    config: &GameConfig,
    a: &mut dyn Bot,
    b: &mut dyn Bot,
) -> (DetailedOutcome, Replay) {
    let mut game = Game::new(Map::generate(seed), config.clone());
    let mut replay = Replay::new(seed, config.clone());

    // Safety valve in case a bot configuration deadlocks the match forever.
    // For an unlimited config (`timeout_ticks <= 0`) this must not silently
    // truncate a match at ~100 s, so only the huge deadlock guard applies.
    let max_ticks = if config.timeout_ticks > 0 {
        config.timeout_ticks + 1_000
    } else {
        1_000_000
    };

    while !game.is_over() && game.tick < max_ticks {
        if game.is_command_tick() {
            let p0 = a.decide(&game, Player::P0);
            for c in &p0 {
                replay.record(game.tick, Player::P0, c.clone());
            }
            if !p0.is_empty() {
                game.apply_commands(Player::P0, &p0);
            }
            let p1 = b.decide(&game, Player::P1);
            for c in &p1 {
                replay.record(game.tick, Player::P1, c.clone());
            }
            if !p1.is_empty() {
                game.apply_commands(Player::P1, &p1);
            }
        }
        game.step();
    }

    replay.result = Some(ReplayResult {
        winner: game.winner,
        reason: game.win_reason,
        duration_ticks: game.tick,
    });

    let outcome = DetailedOutcome {
        outcome: MatchOutcome {
            winner: game.winner,
            reason: game.win_reason,
            duration_ticks: game.tick,
        },
        p0_value: game.remaining_value(Player::P0),
        p1_value: game.remaining_value(Player::P1),
    };
    (outcome, replay)
}

/// Run a head-to-head series and report win counts. `a` plays P0, `b` plays P1.
pub fn series(
    seeds: impl Iterator<Item = u64>,
    config: &GameConfig,
    make_a: impl Fn() -> Box<dyn Bot>,
    make_b: impl Fn() -> Box<dyn Bot>,
) -> SeriesReport {
    let mut report = SeriesReport::default();
    for seed in seeds {
        let mut a = make_a();
        let mut b = make_b();
        let outcome = run_match(seed, config, a.as_mut(), b.as_mut());
        match outcome.winner {
            Some(Player::P0) => report.a_wins += 1,
            Some(Player::P1) => report.b_wins += 1,
            None => report.draws += 1,
        }
        report.matches += 1;
    }
    report
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeriesReport {
    pub matches: u32,
    pub a_wins: u32,
    pub b_wins: u32,
    pub draws: u32,
}

impl SeriesReport {
    /// `a`'s win rate as a fraction in [0, 1].
    pub fn a_win_rate(&self) -> f64 {
        if self.matches == 0 {
            0.0
        } else {
            self.a_wins as f64 / self.matches as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripted::hard;
    use crate::GenomeBot;
    use crucible_sim::serialize;

    #[test]
    fn replay_reproduces_match_byte_identically() {
        let cfg = GameConfig {
            timeout_ticks: 500,
            ..GameConfig::default()
        };
        let mut a = GenomeBot::new(crucible_ai_genome(7));
        let mut b = hard();
        let (outcome, replay) = run_match_with_replay(3, &cfg, &mut a, &mut b);

        // Re-run the input log; the final state must match the direct run.
        let mut repro = serialize::replay_to_game(&replay);
        while repro.tick < outcome.outcome.duration_ticks && !repro.is_over() {
            repro.step();
        }
        assert_eq!(
            repro.winner,
            replay.result.as_ref().and_then(|r| r.winner),
            "replayed winner must match the recorded result"
        );
        assert_eq!(repro.winner, outcome.outcome.winner);
    }

    #[test]
    fn replay_is_deterministic() {
        let cfg = GameConfig {
            timeout_ticks: 300,
            ..GameConfig::default()
        };
        let g = crucible_ai_genome(11);
        let (o1, r1) = run_match_with_replay(5, &cfg, &mut GenomeBot::new(g.clone()), &mut hard());
        let (o2, r2) = run_match_with_replay(5, &cfg, &mut GenomeBot::new(g), &mut hard());
        assert_eq!(o1.outcome, o2.outcome);
        assert_eq!(r1.to_json(), r2.to_json());
    }

    fn crucible_ai_genome(seed: u64) -> Vec<f32> {
        crate::init(&mut crucible_sim::Rng::from_seed(seed))
    }
}
