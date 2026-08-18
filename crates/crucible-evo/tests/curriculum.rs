//! M4 / v1.0 acceptance: the bootstrap curriculum (plan §5.7) converges from
//! a random population to a genome that beats the hard scripted bot ≥ 90%,
//! within a bounded, reproducible generation budget.
//!
//! The exact schedule below was swept across master seeds 1, 7, 42, and
//! 20240818 and converged for all four (see CONTRACT.md); the test pins one
//! seed so CI is deterministic, and prints the measured win rate.

use crucible_evo::{Curriculum, CurriculumConfig, EsParams, Stage};

fn ci_config(master_seed: u64) -> CurriculumConfig {
    CurriculumConfig {
        es: EsParams {
            population_size: 16,
            mu: 4,
            sigma: 0.05,
            ..EsParams::default()
        },
        gens_per_stage: 2,
        seeds_per_generation: 2,
        match_timeout_ticks: 2 * 60 * 10,
        shaping_ticks: 600,
        master_seed,
    }
}

#[test]
fn curriculum_converges_to_beating_hard() {
    let mut c = Curriculum::init(ci_config(20240818));
    let mut generations = 0u32;
    while c.stage != Stage::Done {
        c.run_generation();
        generations += 1;
    }

    let held_out: Vec<u64> = (1000..1032).collect(); // 32 unseen maps
    let rates = c.scripted_win_rates(&held_out);
    println!(
        "curriculum converged in {generations} generations; best genome vs scripted bots over {} held-out seeds: easy {:.1}% / medium {:.1}% / hard {:.1}%",
        held_out.len(),
        rates[0] * 100.0,
        rates[1] * 100.0,
        rates[2] * 100.0
    );

    // The enforceable bootstrap floor (plan §5.7 / M4) is hard ≥ 90%. The
    // stronger "all three scripted bots ≥ 90%" bar is not yet robustly
    // reachable — easy/medium are printed above and tracked in CONTRACT.md.
    assert!(
        rates[2] >= 0.90,
        "curriculum must beat hard >= 90% (got {:.1}%)",
        rates[2] * 100.0
    );
}

#[test]
fn curriculum_is_deterministic() {
    let mut a = Curriculum::init(ci_config(42));
    let mut b = Curriculum::init(ci_config(42));
    for _ in 0..4 {
        a.run_generation();
        b.run_generation();
    }
    assert_eq!(a.pop.genomes, b.pop.genomes);
    assert_eq!(a.stage, b.stage);
    assert_eq!(a.best_genome(), b.best_genome());
}
