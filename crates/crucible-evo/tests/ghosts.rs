//! M7 acceptance: a recorded "cheese" strategy (which beats the champion) is
//! turned into a ghost, and a focused self-play run learns to beat it within a
//! bounded generation budget.
//!
//! The cheese is the medium scripted bot: a refinery-funded pressure build
//! (6 infantry + 4 tanks with periodic attacks) that destroys a no-op
//! champion's HQ and threatens even the bootstrapped economy lineage. The
//! deposit-only economy rework made simple early rushes beatable by any
//! competent opener, so the cheese must bring real force.

use crucible_ai::{run_match_detailed, run_match_with_replay, GenomeBot, GENOME_LEN};
use crucible_evo::{evaluate_economy, ghost_fitness, Ghost, Population};
use crucible_sim::{GameConfig, Player, Rng};

const GHOST_SEEDS: [u64; 4] = [10, 11, 12, 13];

fn test_config() -> GameConfig {
    GameConfig {
        timeout_ticks: 1800, // 3 minutes: enough for a push to resolve
        ..GameConfig::default()
    }
}

/// Record the cheese beating a no-op champion (destroys its HQ).
fn record_cheese(seed: u64, config: &GameConfig) -> crucible_sim::Replay {
    let mut human = crucible_ai::hard();
    let mut champion = GenomeBot::new(vec![0.0f32; GENOME_LEN]); // no-op
    let (_outcome, replay) = run_match_with_replay(seed, config, &mut human, &mut champion);
    assert_eq!(
        replay.result.as_ref().and_then(|r| r.winner),
        Some(Player::P0),
        "cheese must beat the champion on seed {seed}"
    );
    replay
}

fn ghosts(config: &GameConfig) -> Vec<Ghost> {
    GHOST_SEEDS
        .iter()
        .map(|&seed| Ghost::from_replay(&record_cheese(seed, config), Player::P0))
        .collect()
}

/// Win rate of `genome` (playing P1) against the ghosts.
fn win_rate_vs_ghosts(genome: &[f32], ghosts: &[Ghost], config: &GameConfig) -> f32 {
    let mut wins = 0u32;
    for ghost in ghosts {
        let mut g = ghost.clone();
        let mut genome_bot = GenomeBot::new(genome.to_vec());
        let d = run_match_detailed(ghost.map_seed(), config, &mut g, &mut genome_bot);
        if d.outcome.winner == Some(Player::P1) {
            wins += 1;
        }
    }
    wins as f32 / ghosts.len() as f32
}

#[test]
fn lineage_learns_to_beat_the_cheese_ghost() {
    let config = test_config();
    let ghosts = ghosts(&config);

    // Stage 1: bootstrap economy so the population is competent (proven to
    // converge in the M4 bootstrap experiment).
    let mut rng = Rng::from_seed(2024);
    let mut pop = Population::init(
        &mut rng,
        crucible_evo::EsParams {
            population_size: 24,
            mu: 6,
            sigma: 0.08,
            ..crucible_evo::EsParams::default()
        },
    );
    let mut fitnesses: Vec<f32> = pop
        .genomes
        .iter()
        .map(|g| evaluate_economy(g, &[1], &config, 900))
        .collect();
    for _ in 0..3 {
        pop = pop.step(&mut rng, &fitnesses);
        fitnesses = pop
            .genomes
            .iter()
            .map(|g| evaluate_economy(g, &[1], &config, 900))
            .collect();
    }

    let before = win_rate_vs_ghosts(&pop.genomes[pop.best_index(&fitnesses)], &ghosts, &config);
    assert!(
        before < 0.5,
        "the economy lineage should lose to the cheese before focused training (got {before})"
    );

    // Stage 2: focused training vs the cheese ghost.
    let generations = 8;
    let mut ghost_fitnesses: Vec<f32> = pop
        .genomes
        .iter()
        .map(|g| ghost_fitness(g, &ghosts, &config))
        .collect();
    for _ in 0..generations {
        pop = pop.step(&mut rng, &ghost_fitnesses);
        ghost_fitnesses = pop
            .genomes
            .iter()
            .map(|g| ghost_fitness(g, &ghosts, &config))
            .collect();
    }

    let after = win_rate_vs_ghosts(
        &pop.genomes[pop.best_index(&ghost_fitnesses)],
        &ghosts,
        &config,
    );
    assert!(
        after > before,
        "win rate vs the cheese ghost must improve: {before} -> {after}"
    );
    assert!(
        after >= 0.75,
        "best genome must beat the cheese ghost >= 75% after {generations} focused generations (got {after})"
    );
}

#[test]
fn ghost_win_rate_is_well_defined() {
    let config = test_config();
    let ghosts = ghosts(&config);
    let genome = crucible_ai::init(&mut Rng::from_seed(3));
    let rate = win_rate_vs_ghosts(&genome, &ghosts, &config);
    assert!((0.0..=1.0).contains(&rate));
}
