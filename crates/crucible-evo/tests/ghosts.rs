//! M7 acceptance: a recorded "cheese" strategy (which beats the champion) is
//! turned into a ghost, and a focused self-play run learns to beat it within a
//! bounded generation budget.

use crucible_ai::{run_match_detailed, run_match_with_replay, Bot, GenomeBot, GENOME_LEN};
use crucible_evo::{evaluate_economy, ghost_fitness, Ghost, Population};
use crucible_sim::{
    BuildingType, Command, EntityId, Game, GameConfig, Player, Rng, Stance, UnitType,
};

const GHOST_SEEDS: [u64; 4] = [10, 11, 12, 13];

fn test_config() -> GameConfig {
    GameConfig {
        timeout_ticks: 1800, // 3 minutes: enough for a rush to resolve
        ..GameConfig::default()
    }
}

/// A rush "cheese": barracks + four infantry that attack the enemy HQ. Beats a
/// no-op champion (and an economy-only lineage) but dies to any early defense.
#[derive(Default)]
struct RushBot {
    attacked: bool,
}

fn free_build_tile(game: &Game, player: Player) -> Option<(u8, u8)> {
    let (hx, hy) = game.hq(player)?.tile;
    for dy in 1..6i32 {
        for dx in 1..6i32 {
            let t = (
                (hx as i32 + dx).clamp(0, 63) as u8,
                (hy as i32 + dy).clamp(0, 63) as u8,
            );
            if game.map.is_passable(t.0, t.1)
                && game.map.ore_at(t.0, t.1) == 0
                && !game.buildings.iter().any(|b| b.tile == t)
            {
                return Some(t);
            }
        }
    }
    None
}

impl Bot for RushBot {
    fn name(&self) -> &'static str {
        "rush"
    }
    fn decide(&mut self, game: &Game, player: Player) -> Vec<Command> {
        let has_barracks = game
            .buildings
            .iter()
            .any(|b| b.owner == player && b.btype == BuildingType::Barracks);
        if !has_barracks {
            if let Some(tile) = free_build_tile(game, player) {
                return vec![Command::PlaceBuilding {
                    player,
                    btype: BuildingType::Barracks,
                    tile,
                }];
            }
            return vec![];
        }

        let infantry: Vec<EntityId> = game
            .units
            .iter()
            .filter(|u| u.owner == player && u.utype == UnitType::Infantry)
            .map(|u| u.id)
            .collect();
        if infantry.len() < 4 {
            if let Some(b) = game
                .buildings
                .iter()
                .find(|b| b.owner == player && b.btype == BuildingType::Barracks)
            {
                if b.queue.len() < game.config.max_queue {
                    return vec![Command::TrainUnit {
                        player,
                        building: b.id,
                        utype: UnitType::Infantry,
                    }];
                }
            }
            return vec![];
        }

        if !self.attacked {
            self.attacked = true;
            if let Some(t) = game.hq(player.enemy()).map(|b| b.tile) {
                return vec![Command::MoveGroup {
                    player,
                    units: infantry,
                    waypoint: t,
                    stance: Stance::Aggressive,
                }];
            }
        }
        vec![]
    }
}

/// Record the rush beating a no-op champion (destroys its HQ).
fn record_cheese(seed: u64, config: &GameConfig) -> crucible_sim::Replay {
    let mut human = RushBot::default();
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
            population_size: 16,
            mu: 4,
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
        "the economy lineage should lose to the rush before focused training (got {before})"
    );

    // Stage 2: focused training vs the rush ghost.
    let generations = 6;
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
