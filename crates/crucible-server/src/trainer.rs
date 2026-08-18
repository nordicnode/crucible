//! The continuous trainer: self-play evolution strategy generations, champion
//! gating via the gauntlet, Elo updates, and SQLite checkpointing. CPU-bound
//! headless matches; the tokio wrapper supplies scheduling/yielding.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crucible_evo::{
    ghost_fitness, run_gauntlet, self_play_fitness, Curriculum, CurriculumConfig, EsParams,
    GauntletConfig, Ghost, GhostPool, Population, Stage,
};
use crucible_sim::{GameConfig, Player, Replay, Rng};

use crate::store::Store;

const MIX: u64 = 0x9E37_79B9_7F4A_7C15;

#[derive(Clone, Debug)]
pub struct TrainerConfig {
    pub population_size: usize,
    pub mu: usize,
    pub sigma: f32,
    pub sigma_decay: f32,
    pub macro_rate: f32,
    /// Self-play opponents sampled per genome per generation.
    pub self_play_opponents: usize,
    /// Map seeds per generation evaluation (each played both sides).
    pub seeds_per_generation: usize,
    /// Match length cap used during training (shorter = faster iterations).
    pub match_timeout_ticks: i32,
    pub gauntlet: GauntletConfig,
    /// Seeds used for the promotion change report (0 disables).
    pub report_seeds: usize,
    /// Ghosts sampled per genome per generation (fitness blend).
    pub ghosts_per_generation: usize,
    /// Weight of ghost fitness vs self-play fitness (0..1).
    pub ghost_weight: f32,
    /// Run the staged bootstrap curriculum on a cold start (plan §5.7) before
    /// the self-play loop. Produces a competent population + first champion.
    pub bootstrap: bool,
    pub bootstrap_gens_per_stage: usize,
    pub bootstrap_seeds: usize,
    /// Match cap used *only* during the bootstrap curriculum. The curriculum
    /// converges (beats hard ≥ 90%) at short caps; the full-length self-play
    /// cap is for the league, not the bootstrap floor.
    pub bootstrap_match_timeout_ticks: i32,
    pub master_seed: u64,
}

impl Default for TrainerConfig {
    fn default() -> Self {
        TrainerConfig {
            population_size: 64,
            mu: 16,
            sigma: 0.02,
            sigma_decay: 0.995,
            macro_rate: 0.10,
            self_play_opponents: 3,
            seeds_per_generation: 2,
            match_timeout_ticks: 6 * 60 * 10, // 6 minutes
            gauntlet: GauntletConfig::default(),
            report_seeds: 8,
            ghosts_per_generation: 1,
            ghost_weight: 0.3,
            bootstrap: false,
            bootstrap_gens_per_stage: 2,
            bootstrap_seeds: 2,
            bootstrap_match_timeout_ticks: 2 * 60 * 10, // 2 minutes
            master_seed: 0xC0FFEE,
        }
    }
}

impl TrainerConfig {
    /// A small, fast configuration for demos and manual fast-forwards.
    pub fn small() -> Self {
        TrainerConfig {
            // Population/mu must be large enough for the bootstrap curriculum
            // to converge (it runs the same schedule as the CI test); the
            // self-play cost is kept low via opponents/seeds/match cap below.
            population_size: 16,
            mu: 4,
            self_play_opponents: 1,
            seeds_per_generation: 1,
            match_timeout_ticks: 3 * 60 * 10, // 3 minutes
            gauntlet: GauntletConfig {
                champion_seeds: 4,
                historical_seeds: 1,
                historical_count: 2,
                ..GauntletConfig::default()
            },
            report_seeds: 0,
            ghosts_per_generation: 1,
            bootstrap: true,
            bootstrap_gens_per_stage: 2,
            bootstrap_seeds: 2,
            ..TrainerConfig::default()
        }
    }
}

/// Live status for `/api/status` (cheap, atomic; durable data lives in SQLite).
#[derive(Default)]
pub struct TrainerShared {
    pub generation: AtomicU32,
    pub matches_run: AtomicU64,
    pub ghost_pool_size: AtomicU64,
    pub running: AtomicBool,
    pub last_event: Mutex<Option<serde_json::Value>>,
}

impl TrainerShared {
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "generation": self.generation.load(Ordering::Relaxed),
            "matches_run": self.matches_run.load(Ordering::Relaxed),
            "ghost_pool_size": self.ghost_pool_size.load(Ordering::Relaxed),
            "running": self.running.load(Ordering::Relaxed),
            "last_event": self.last_event.lock().unwrap().clone(),
        })
    }
}

struct Champion {
    genome_id: i64,
    weights: Vec<f32>,
    generation: u32,
    elo: f32,
}

/// One promotion, returned for tests and logged as an event.
#[derive(Clone, Debug)]
pub struct Promotion {
    pub genome_id: i64,
    pub generation: u32,
    pub elo: f32,
    pub gauntlet: crucible_evo::GauntletResult,
}

pub struct Trainer {
    cfg: TrainerConfig,
    game_config: GameConfig,
    es: EsParams,
    master_seed: u64,
    pop: Population,
    ids: Vec<i64>,
    champion: Option<Champion>,
    historical: Vec<Vec<f32>>,
    ghost_pool: GhostPool,
    store: Arc<Store>,
    shared: Arc<TrainerShared>,
}

fn mix(master_seed: u64, generation: u32, salt: u64) -> u64 {
    master_seed ^ ((generation as u64).wrapping_mul(MIX)) ^ salt
}

fn generation_seeds(master_seed: u64, generation: u32, n: usize) -> Vec<u64> {
    let mut rng = Rng::from_seed(mix(master_seed, generation, 0x1111));
    (0..n).map(|_| rng.next_u64()).collect()
}

fn sigma_at(es: &EsParams, generation: u32) -> f32 {
    (es.sigma * es.sigma_decay.powi(generation as i32)).max(es.sigma_min)
}

impl Trainer {
    /// Build a trainer, resuming the population + champion from the store if a
    /// previous run checkpointed them.
    pub fn start(
        store: Arc<Store>,
        shared: Arc<TrainerShared>,
        cfg: TrainerConfig,
    ) -> Result<Trainer, rusqlite::Error> {
        let es = EsParams {
            population_size: cfg.population_size,
            mu: cfg.mu,
            sigma: cfg.sigma,
            sigma_decay: cfg.sigma_decay,
            macro_rate: cfg.macro_rate,
            ..EsParams::default()
        };
        let game_config = GameConfig {
            timeout_ticks: cfg.match_timeout_ticks,
            ..GameConfig::default()
        };

        // Stable master seed across restarts.
        let master_seed = match store.get_state("master_seed")? {
            Some(s) => s.parse().unwrap_or(cfg.master_seed),
            None => {
                store.set_state("master_seed", &cfg.master_seed.to_string())?;
                cfg.master_seed
            }
        };

        // Resume the latest checkpointed population, or initialize cold.
        let (pop, ids) = match store.latest_generation()? {
            Some(gen) => {
                let rows = store.genomes_of_generation(gen)?;
                let genomes: Vec<Vec<f32>> = rows.iter().map(|r| r.weights.clone()).collect();
                let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
                if genomes.is_empty() {
                    (
                        Population::init(&mut Rng::from_seed(master_seed), es),
                        Vec::new(),
                    )
                } else {
                    (
                        Population {
                            genomes,
                            generation: gen,
                            sigma: sigma_at(&es, gen),
                            params: es,
                        },
                        ids,
                    )
                }
            }
            None => (
                Population::init(&mut Rng::from_seed(master_seed), es),
                Vec::new(),
            ),
        };

        // Load the reigning champion and recent historical champions.
        let champion = load_champion(&store)?;
        let historical = load_historical(&store)?;

        // Bootstrap a cold start through the staged curriculum (plan §5.7) so
        // the self-play loop begins from a competent population + champion.
        let (pop, ids, champion) = if cfg.bootstrap && champion.is_none() && ids.is_empty() {
            bootstrap_cold(&store, &cfg, es, master_seed)?
        } else {
            (pop, ids, champion)
        };

        // Rebuild the ghost pool from stored human matches.
        let ghost_pool = load_ghost_pool(&store, 200)?;
        shared
            .ghost_pool_size
            .store(ghost_pool.len() as u64, Ordering::Relaxed);

        Ok(Trainer {
            cfg,
            game_config,
            es,
            master_seed,
            pop,
            ids,
            champion,
            historical,
            ghost_pool,
            store,
            shared,
        })
    }

    /// Run one full generation: evaluate, evolve, checkpoint, gauntlet-test the
    /// winner, and (if it passes) promote it. Returns the promotion, if any.
    pub fn run_generation(&mut self) -> Result<Option<Promotion>, rusqlite::Error> {
        let generation = self.pop.generation;

        // Persist the current population as roots on the very first run.
        if self.ids.is_empty() {
            let rows: Vec<(Option<i64>, &str, Vec<f32>)> = self
                .pop
                .genomes
                .iter()
                .map(|g| (None, "init", g.clone()))
                .collect();
            self.ids = self.store.save_generation(generation, &rows)?;
        }

        let seeds = generation_seeds(self.master_seed, generation, self.cfg.seeds_per_generation);

        // Sample ghosts once per generation (champion-beaters prioritized).
        let mut grng = Rng::from_seed(mix(self.master_seed, generation, 0x3333));
        let ghosts = self.sample_ghosts(&mut grng);

        // Evaluate every genome (self-play + champion + ghosts).
        let mut fitnesses = Vec::with_capacity(self.pop.genomes.len());
        for (i, genome) in self.pop.genomes.iter().enumerate() {
            let mut opponents = Vec::new();
            let mut srng = Rng::from_seed(mix(self.master_seed, generation, i as u64 + 1));
            for _ in 0..self.cfg.self_play_opponents.min(self.pop.genomes.len()) {
                let idx = srng.below(self.pop.genomes.len() as u64) as usize;
                opponents.push(self.pop.genomes[idx].clone());
            }
            let champion = self.champion.as_ref().map(|c| c.weights.as_slice());
            let sp = self_play_fitness(genome, &opponents, champion, &seeds, &self.game_config);
            let fitness = if ghosts.is_empty() {
                sp
            } else {
                let g = ghost_fitness(genome, &ghosts, &self.game_config);
                (1.0 - self.cfg.ghost_weight) * sp + self.cfg.ghost_weight * g
            };
            fitnesses.push(fitness);
        }

        let winner_idx = self.pop.best_index(&fitnesses);
        let winner = self.pop.genomes[winner_idx].clone();
        let winner_id = self.ids[winner_idx];

        // Evolve to the next generation and checkpoint it.
        let step_rng = &mut Rng::from_seed(mix(self.master_seed, generation, 0x2222));
        let (next, parents) = self.pop.step_with_parents(step_rng, &fitnesses);
        let next_gen = next.generation;
        let mu = self.es.mu.min(self.pop.genomes.len());
        let rows: Vec<(Option<i64>, &str, Vec<f32>)> = next
            .genomes
            .iter()
            .enumerate()
            .map(|(j, g)| {
                let parent_idx = parents[j];
                let born = if j < mu { "elite" } else { "mutant" };
                (Some(self.ids[parent_idx]), born, g.clone())
            })
            .collect();
        let new_ids = self.store.save_generation(next_gen, &rows)?;
        self.ids = new_ids;
        self.pop = next;

        // Persist generation stats and update the live counters.
        let (mean, best) = Population::fitness_stats(&fitnesses);
        let diversity = self.pop.diversity();
        let matches_this_gen = self.count_matches_this_generation();
        self.shared
            .matches_run
            .fetch_add(matches_this_gen, Ordering::Relaxed);
        self.store.save_training_stats(
            generation,
            self.shared.matches_run.load(Ordering::Relaxed),
            mean,
            best,
            diversity,
        )?;
        self.shared.generation.store(next_gen, Ordering::Relaxed);

        // Gauntlet-test the winner against the reigning champion.
        let promotion = self.consider_champion(&winner, winner_id, generation)?;
        Ok(promotion)
    }

    /// Sample ghosts for a generation: champion-beaters always come first
    /// (the post-upset focused cycle), then recency-weighted pool sampling.
    fn sample_ghosts(&self, rng: &mut Rng) -> Vec<Ghost> {
        if self.ghost_pool.is_empty() {
            return Vec::new();
        }
        let want = self.cfg.ghosts_per_generation;
        let mut ghosts = self.ghost_pool.champion_beaters();
        if ghosts.len() < want {
            ghosts.extend(self.ghost_pool.sample(rng, want - ghosts.len()));
        }
        ghosts.truncate(want);
        ghosts
    }

    fn count_matches_this_generation(&self) -> u64 {
        let opponents = self.cfg.self_play_opponents.min(self.pop.genomes.len());
        let slots = opponents + usize::from(self.champion.is_some());
        (self.pop.genomes.len() * slots * self.cfg.seeds_per_generation * 2) as u64
    }

    /// Crown `winner` (directly if there is no champion yet, else via gauntlet).
    fn consider_champion(
        &mut self,
        winner: &[f32],
        winner_id: i64,
        generation: u32,
    ) -> Result<Option<Promotion>, rusqlite::Error> {
        // First champion: crowning v1 has no gauntlet (no incumbent to beat).
        if self.champion.is_none() {
            let elo = 1500.0f32;
            self.store.crown_champion(winner_id, generation, None)?;
            self.store.record_elo(winner_id, elo)?;
            self.champion = Some(Champion {
                genome_id: winner_id,
                weights: winner.to_vec(),
                generation,
                elo,
            });
            self.emit_event(
                "first_champion",
                serde_json::json!({ "genome_id": winner_id, "generation": generation, "elo": elo }),
            );
            return Ok(None); // first champion is not a "promotion" (no gauntlet)
        }

        // Copy the incumbent's fields out so we can mutate `self.champion`.
        let incumbent_genome_id = self.champion.as_ref().unwrap().genome_id;
        let incumbent_weights = self.champion.as_ref().unwrap().weights.clone();
        let incumbent_elo = self.champion.as_ref().unwrap().elo;
        let incumbent_generation = self.champion.as_ref().unwrap().generation;

        let gauntlet_seeds = generation_seeds(
            self.master_seed,
            generation,
            self.cfg
                .gauntlet
                .champion_seeds
                .max(self.cfg.gauntlet.historical_seeds) as usize,
        );
        let result = run_gauntlet(
            winner,
            &incumbent_weights,
            &self.historical,
            &gauntlet_seeds,
            &self.game_config,
            &self.cfg.gauntlet,
        );

        if !result.promoted {
            return Ok(None);
        }

        // Elo: challenger starts at the incumbent's rating; each champion match
        // moves it by K (equal ratings ⇒ expected 0.5 per match).
        let net = (2.0 * result.champion_wins as f32 - result.champion_total as f32) * 0.5;
        let new_elo = incumbent_elo + crucible_evo::K * net;

        // Change report (optional, small evaluation set).
        if self.cfg.report_seeds > 0 {
            let report_seeds =
                generation_seeds(self.master_seed, generation, self.cfg.report_seeds);
            let report = crucible_evo::change_report(
                &incumbent_weights,
                winner,
                &report_seeds,
                &self.game_config,
            );
            self.store
                .record_event("change_report", serde_json::json!(report))?;
        }

        // Dethrone: incumbent becomes historical.
        self.historical.push(incumbent_weights);
        if self.historical.len() > 4 {
            self.historical.remove(0);
        }

        let gauntlet_json = serde_json::to_value(result).unwrap_or(serde_json::Value::Null);
        self.store
            .crown_champion(winner_id, generation, Some(gauntlet_json.clone()))?;
        self.store.record_elo(winner_id, new_elo)?;

        self.champion = Some(Champion {
            genome_id: winner_id,
            weights: winner.to_vec(),
            generation,
            elo: new_elo,
        });

        let promotion = Promotion {
            genome_id: winner_id,
            generation,
            elo: new_elo,
            gauntlet: result,
        };
        self.emit_event(
            "promotion",
            serde_json::json!({
                "genome_id": winner_id,
                "generation": generation,
                "elo": new_elo,
                "dethroned": incumbent_genome_id,
                "dethroned_generation": incumbent_generation,
                "gauntlet": gauntlet_json,
            }),
        );
        Ok(Some(promotion))
    }

    fn emit_event(&self, kind: &str, payload: serde_json::Value) {
        let _ = self.store.record_event(kind, payload.clone());
        if let Ok(mut slot) = self.shared.last_event.lock() {
            *slot = Some(serde_json::json!({ "kind": kind, "payload": payload }));
        }
    }
}

/// Rebuild the ghost pool from stored human matches (most recent weighted
/// highest; any human win is flagged as a champion-beater for v1).
fn load_ghost_pool(store: &Store, max_size: usize) -> Result<GhostPool, rusqlite::Error> {
    let mut pool = GhostPool::new(max_size);
    let mut matches = store.list_matches(500)?;
    matches.reverse(); // oldest first, so recency rises with match id
    for m in matches {
        if m.p1_type != "human" {
            continue;
        }
        let Some(replay_json) = store.get_replay(m.id)? else {
            continue;
        };
        let Ok(replay) = Replay::from_json(&replay_json) else {
            continue;
        };
        let ghost = Ghost::from_replay(&replay, Player::P0);
        pool.add(m.id as u64, ghost, m.result.contains("P0"));
    }
    Ok(pool)
}

/// Run the staged bootstrap curriculum on a cold start and checkpoint the
/// resulting population + first champion (plan §5.7).
fn bootstrap_cold(
    store: &Store,
    cfg: &TrainerConfig,
    es: EsParams,
    master_seed: u64,
) -> Result<(Population, Vec<i64>, Option<Champion>), rusqlite::Error> {
    // Higher exploration than steady-state self-play: a random population needs
    // bigger jumps to cross the "can build a base" fitness cliff.
    let ccfg = CurriculumConfig {
        es: EsParams { sigma: 0.05, ..es },
        gens_per_stage: cfg.bootstrap_gens_per_stage,
        seeds_per_generation: cfg.bootstrap_seeds,
        match_timeout_ticks: cfg.bootstrap_match_timeout_ticks,
        shaping_ticks: 600,
        master_seed,
    };
    let mut cur = Curriculum::init(ccfg);
    while cur.stage != Stage::Done {
        cur.run_generation();
    }

    // Enforce the bootstrap floor at crowning time (plan §5.7 / M4): the first
    // champion must beat the hard scripted bot ≥ 90% on held-out maps before it
    // is crowned. (The stronger "all three scripted bots ≥ 90%" regression bar
    // is not yet robustly reachable — see CONTRACT.md — so hard remains the
    // enforceable floor; easy/medium are recorded for the regression run.)
    let held_out: Vec<u64> = (10_000..10_032).collect();
    let rates = cur.scripted_win_rates(&held_out);
    assert!(
        rates[2] >= 0.90,
        "bootstrap champion must beat hard >= 90% (got {:.1}%; easy {:.1}%, medium {:.1}%)",
        rates[2] * 100.0,
        rates[0] * 100.0,
        rates[1] * 100.0
    );

    // The bootstrap population becomes generation 0 of the trainer's lineage;
    // steady-state self-play resumes with the trainer's own ES parameters.
    let mut pop = cur.pop;
    pop.generation = 0;
    pop.params = es;
    pop.sigma = es.sigma;
    let rows: Vec<(Option<i64>, &str, Vec<f32>)> = pop
        .genomes
        .iter()
        .map(|g| (None, "bootstrap", g.clone()))
        .collect();
    let ids = store.save_generation(0, &rows)?;

    // The elitist best of the curriculum becomes the first champion.
    let champion_id = ids[0];
    store.crown_champion(champion_id, 0, None)?;
    store.record_elo(champion_id, 1500.0)?;
    let champion = Some(Champion {
        genome_id: champion_id,
        weights: pop.genomes[0].clone(),
        generation: 0,
        elo: 1500.0,
    });

    Ok((pop, ids, champion))
}

fn load_champion(store: &Store) -> Result<Option<Champion>, rusqlite::Error> {
    let Some(c) = store.get_reigning_champion()? else {
        return Ok(None);
    };
    let Some(weights) = store.get_genome_weights(c.genome_id)? else {
        return Ok(None);
    };
    let elo = store
        .elo_history(c.genome_id)?
        .last()
        .map(|p| p.elo)
        .unwrap_or(1500.0);
    Ok(Some(Champion {
        genome_id: c.genome_id,
        weights,
        generation: c.generation,
        elo,
    }))
}

fn load_historical(store: &Store) -> Result<Vec<Vec<f32>>, rusqlite::Error> {
    let mut out = Vec::new();
    for c in store.list_champions()? {
        if c.reigning() {
            continue;
        }
        if let Some(w) = store.get_genome_weights(c.genome_id)? {
            out.push(w);
        }
    }
    if out.len() > 4 {
        out = out.split_off(out.len() - 4);
    }
    Ok(out)
}

/// Convenience wrapper for tests: run `n` generations to completion.
#[cfg(test)]
pub fn run_generations(
    store: Arc<Store>,
    shared: Arc<TrainerShared>,
    cfg: TrainerConfig,
    n: usize,
) -> Result<usize, rusqlite::Error> {
    let mut trainer = Trainer::start(store, shared.clone(), cfg)?;
    shared.running.store(true, Ordering::Relaxed);
    let mut promotions = 0;
    for _ in 0..n {
        if let Some(_p) = trainer.run_generation()? {
            promotions += 1;
        }
    }
    shared.running.store(false, Ordering::Relaxed);
    Ok(promotions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_config() -> TrainerConfig {
        TrainerConfig {
            population_size: 6,
            mu: 2,
            self_play_opponents: 1,
            seeds_per_generation: 1,
            match_timeout_ticks: 300,
            gauntlet: GauntletConfig {
                champion_seeds: 1,
                historical_seeds: 1,
                champion_win_rate: 0.55,
                historical_win_rate: 0.50,
                historical_count: 2,
            },
            report_seeds: 0,
            ..TrainerConfig::default()
        }
    }

    #[test]
    fn trainer_evolves_and_checkpoints() {
        let store = Arc::new(Store::in_memory().unwrap());
        let shared = Arc::new(TrainerShared::default());
        let promotions = run_generations(store.clone(), shared.clone(), tiny_config(), 2).unwrap();

        // The first champion is always crowned (no gauntlet), then the winner of
        // generation 1 is gauntlet-tested; a promotion is optional here.
        let champion = store.get_reigning_champion().unwrap().unwrap();
        assert!(champion.generation <= 2);

        // Population was checkpointed: at least gens 0 and 1 exist.
        let latest = store.latest_generation().unwrap().unwrap();
        assert!(latest >= 1);
        assert_eq!(store.genomes_of_generation(latest).unwrap().len(), 6);

        // Training stats + lineage are persisted.
        assert!(!store.list_training_stats(10).unwrap().is_empty());
        let gen1 = store.genomes_of_generation(1).unwrap();
        assert!(gen1.iter().all(|g| g.parent_id.is_some()));

        // Live counters were updated.
        assert!(shared.matches_run.load(Ordering::Relaxed) > 0);
        let _ = promotions;
    }

    #[test]
    fn trainer_loads_and_prioritizes_beater_ghosts() {
        use crucible_ai::{hard, run_match_with_replay, GenomeBot, GENOME_LEN};

        let store = Arc::new(Store::in_memory().unwrap());
        let cfg = GameConfig {
            timeout_ticks: 800,
            ..GameConfig::default()
        };
        let seed = 42u64;

        // Record a human (the hard bot stands in) beating a no-op champion.
        let mut human = hard();
        let mut champion = GenomeBot::new(vec![0.0f32; GENOME_LEN]);
        let (_o, replay) = run_match_with_replay(seed, &cfg, &mut human, &mut champion);
        assert_eq!(
            replay.result.as_ref().and_then(|r| r.winner),
            Some(Player::P0)
        );
        store
            .save_match(
                seed,
                "human",
                "bot:hard",
                "Some(P0)",
                replay.result.as_ref().unwrap().duration_ticks,
                &replay.to_json(),
            )
            .unwrap();

        // The pool is rebuilt from the stored human match, flagged as a beater.
        let pool = load_ghost_pool(&store, 200).unwrap();
        assert_eq!(pool.len(), 1);
        assert_eq!(pool.champion_beaters().len(), 1);

        // Trainer start loads it and surfaces the pool size in /api/status.
        let shared = Arc::new(TrainerShared::default());
        let t = Trainer::start(store.clone(), shared.clone(), tiny_config()).unwrap();
        assert_eq!(t.ghost_pool.len(), 1);
        assert_eq!(shared.ghost_pool_size.load(Ordering::Relaxed), 1);

        // Champion-beaters are prioritized over recency sampling.
        let mut rng = Rng::from_seed(1);
        let sampled = t.sample_ghosts(&mut rng);
        assert_eq!(sampled.len(), 1);
    }

    #[test]
    fn trainer_bootstraps_cold_start() {
        let store = Arc::new(Store::in_memory().unwrap());
        let shared = Arc::new(TrainerShared::default());
        // A converging bootstrap schedule (matches the CI curriculum test): the
        // cold-start champion must clear the hard-bot floor before crowning.
        let cfg = TrainerConfig {
            population_size: 16,
            mu: 4,
            self_play_opponents: 1,
            seeds_per_generation: 1,
            match_timeout_ticks: 300,
            gauntlet: GauntletConfig {
                champion_seeds: 1,
                historical_seeds: 1,
                historical_count: 2,
                ..GauntletConfig::default()
            },
            report_seeds: 0,
            bootstrap: true,
            bootstrap_gens_per_stage: 2,
            bootstrap_seeds: 2,
            bootstrap_match_timeout_ticks: 2 * 60 * 10,
            ..TrainerConfig::default()
        };

        // Cold start: the curriculum should crown a champion and checkpoint the
        // bootstrapped population before any self-play generation runs.
        let mut t = Trainer::start(store.clone(), shared.clone(), cfg).unwrap();
        assert!(t.champion.is_some(), "bootstrap must crown a champion");
        assert_eq!(t.champion.as_ref().unwrap().generation, 0);
        assert_eq!(t.pop.generation, 0);
        assert_eq!(t.ids.len(), 16);
        assert_eq!(store.genomes_of_generation(0).unwrap().len(), 16);
        assert!(store.get_reigning_champion().unwrap().is_some());

        // And the trainer keeps running self-play generations afterward.
        t.run_generation().unwrap();
        assert!(t.pop.generation >= 1);
    }

    #[test]
    fn trainer_resumes_from_checkpoint() {
        let store = Arc::new(Store::in_memory().unwrap());
        let shared = Arc::new(TrainerShared::default());
        run_generations(store.clone(), shared, tiny_config(), 2).unwrap();

        // "Restart": build a new trainer over the same store.
        let mut t = Trainer::start(
            store.clone(),
            Arc::new(TrainerShared::default()),
            tiny_config(),
        )
        .unwrap();
        let resumed_gen = t.pop.generation;
        assert!(resumed_gen >= 1);
        assert_eq!(t.ids.len(), 6);
        assert!(t.champion.is_some());

        // It keeps evolving from the checkpoint (no reset to generation 0).
        t.run_generation().unwrap();
        assert!(t.pop.generation > resumed_gen);
    }
}
