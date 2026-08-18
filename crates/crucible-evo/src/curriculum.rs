//! The bootstrap curriculum (plan §5.7): staged evolution from a random
//! population to a genome that beats the hard scripted bot. Pure — the caller
//! supplies seeds/config and drives generations; no IO or scheduling.
//!
//! Stages, in order: economy (ore mined) → production (army value) → combat
//! (vs idle) → scripted easy → medium → hard. Each stage runs a *bounded*
//! number of ES generations and then advances, so the whole schedule is a
//! fixed, reproducible budget. The final measurement is the best genome's win
//! rate against `hard` on held-out seeds.

use crucible_ai::{easy, hard, medium, series, Bot, GenomeBot};
use crucible_sim::{GameConfig, Rng};

use crate::fitness::{evaluate_economy, evaluate_production, evaluate_vs, Noop};
use crate::population::{EsParams, Population};

const MIX: u64 = 0x9E37_79B9_7F4A_7C15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Economy,
    Production,
    Combat,
    ScriptedEasy,
    ScriptedMedium,
    ScriptedHard,
    Done,
}

impl Stage {
    pub fn next(self) -> Stage {
        match self {
            Stage::Economy => Stage::Production,
            Stage::Production => Stage::Combat,
            Stage::Combat => Stage::ScriptedEasy,
            Stage::ScriptedEasy => Stage::ScriptedMedium,
            Stage::ScriptedMedium => Stage::ScriptedHard,
            Stage::ScriptedHard => Stage::Done,
            Stage::Done => Stage::Done,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Economy => "economy",
            Stage::Production => "production",
            Stage::Combat => "combat",
            Stage::ScriptedEasy => "scripted-easy",
            Stage::ScriptedMedium => "scripted-medium",
            Stage::ScriptedHard => "scripted-hard",
            Stage::Done => "done",
        }
    }

    fn id(self) -> u64 {
        match self {
            Stage::Economy => 0,
            Stage::Production => 1,
            Stage::Combat => 2,
            Stage::ScriptedEasy => 3,
            Stage::ScriptedMedium => 4,
            Stage::ScriptedHard => 5,
            Stage::Done => 6,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CurriculumConfig {
    pub es: EsParams,
    /// Generations to run in each stage before advancing.
    pub gens_per_stage: usize,
    pub seeds_per_generation: usize,
    /// Match cap for opponent-based stages (combat + scripted).
    pub match_timeout_ticks: i32,
    /// Tick cap for the solo shaping stages (economy + production).
    pub shaping_ticks: i32,
    pub master_seed: u64,
}

impl Default for CurriculumConfig {
    fn default() -> Self {
        CurriculumConfig {
            es: EsParams::default(),
            gens_per_stage: 4,
            seeds_per_generation: 2,
            match_timeout_ticks: 3 * 60 * 10,
            shaping_ticks: 600,
            master_seed: 0xB007_57A6,
        }
    }
}

fn mix(master_seed: u64, stage: Stage, generation: u32) -> u64 {
    master_seed ^ (stage.id().wrapping_mul(MIX)) ^ (generation as u64).wrapping_mul(MIX >> 1)
}

pub struct Curriculum {
    pub pop: Population,
    pub stage: Stage,
    pub gens_in_stage: usize,
    cfg: CurriculumConfig,
    match_config: GameConfig,
    shaping_config: GameConfig,
}

impl Curriculum {
    pub fn init(cfg: CurriculumConfig) -> Self {
        let mut rng = Rng::from_seed(cfg.master_seed);
        let pop = Population::init(&mut rng, cfg.es);
        Curriculum {
            pop,
            stage: Stage::Economy,
            gens_in_stage: 0,
            match_config: GameConfig {
                timeout_ticks: cfg.match_timeout_ticks,
                ..GameConfig::default()
            },
            shaping_config: GameConfig {
                timeout_ticks: 100_000,
                ..GameConfig::default()
            },
            cfg,
        }
    }

    fn generation_seeds(&self) -> Vec<u64> {
        let mut rng = Rng::from_seed(mix(self.cfg.master_seed, self.stage, self.pop.generation));
        (0..self.cfg.seeds_per_generation)
            .map(|_| rng.next_u64())
            .collect()
    }

    /// The fitness signal for the current stage (see plan §5.7).
    pub fn evaluate(&self, genome: &[f32]) -> f32 {
        let seeds = self.generation_seeds();
        match self.stage {
            Stage::Economy => {
                evaluate_economy(genome, &seeds, &self.shaping_config, self.cfg.shaping_ticks)
            }
            Stage::Production => {
                evaluate_production(genome, &seeds, &self.shaping_config, self.cfg.shaping_ticks)
            }
            Stage::Combat => evaluate_vs(genome, &seeds, &self.match_config, || -> Box<dyn Bot> {
                Box::new(Noop)
            }),
            Stage::ScriptedEasy => {
                evaluate_vs(genome, &seeds, &self.match_config, || -> Box<dyn Bot> {
                    Box::new(easy())
                })
            }
            Stage::ScriptedMedium => {
                evaluate_vs(genome, &seeds, &self.match_config, || -> Box<dyn Bot> {
                    Box::new(medium())
                })
            }
            Stage::ScriptedHard => {
                evaluate_vs(genome, &seeds, &self.match_config, || -> Box<dyn Bot> {
                    Box::new(hard())
                })
            }
            Stage::Done => 0.0,
        }
    }

    /// Run one ES generation under the current stage, advancing the stage when
    /// its bounded budget is spent. Returns the generation's (mean, best)
    /// fitness under that stage's signal.
    pub fn run_generation(&mut self) -> (f32, f32) {
        if self.stage == Stage::Done {
            return (0.0, 0.0);
        }
        let fitnesses: Vec<f32> = self.pop.genomes.iter().map(|g| self.evaluate(g)).collect();
        let (mean, best) = Population::fitness_stats(&fitnesses);
        let mut rng = Rng::from_seed(
            mix(self.cfg.master_seed, self.stage, self.pop.generation)
                .wrapping_add(0x1234_5678_9ABC_DEF0),
        );
        self.pop = self.pop.step(&mut rng, &fitnesses);
        self.gens_in_stage += 1;
        if self.gens_in_stage >= self.cfg.gens_per_stage {
            self.stage = self.stage.next();
            self.gens_in_stage = 0;
        }
        (mean, best)
    }

    /// The elitist best genome produced so far (elites are sorted best-first).
    pub fn best_genome(&self) -> Vec<f32> {
        self.pop.genomes[0].clone()
    }

    /// The best genome's win rate against the hard bot over held-out seeds.
    pub fn hard_win_rate(&self, seeds: &[u64]) -> f32 {
        let genome = self.best_genome();
        let report = series(
            seeds.iter().copied(),
            &self.match_config,
            || -> Box<dyn Bot> { Box::new(GenomeBot::new(genome.clone())) },
            || -> Box<dyn Bot> { Box::new(hard()) },
        );
        report.a_win_rate() as f32
    }

    /// Run the whole schedule to completion (through `ScriptedHard`). Returns
    /// the best genome's win rate vs hard over `held_out`.
    pub fn run_to_completion(&mut self, held_out: &[u64]) -> f32 {
        while self.stage != Stage::Done {
            self.run_generation();
        }
        self.hard_win_rate(held_out)
    }
}
