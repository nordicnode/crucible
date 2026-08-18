# CONTRACT — crucible-evo

Pure training logic: the (μ+λ) evolution strategy, fitness, lineage, ghosts,
the champion gauntlet, Elo, and change reports. Depends on `crucible-sim` and
`crucible-ai` only. **Status: M4 (population + fitness) + M5 (gauntlet, lineage,
league/Elo, change reports) + M7 (ghosts, ghost pool, ghost fitness)
implemented.**

## 1. Purity boundary

`crucible-evo` MUST NOT do IO, spawn threads, or read the clock. It computes:
given a population, an evaluation set, and match results, produce the next
population / fitness / lineage updates. The server injects storage, scheduling,
parallelism (rayon), and match execution.

## 2. Evolution strategy contract

- (μ+λ) ES, mutation-only in v1 (no crossover — keeps lineage trees clean):
  population 64, μ = top 16 retained, λ = 48 offspring via Gaussian mutation,
  σ annealed 0.02 → 0.005 by generation, 10% macromutation rate.
- Fitness per genome = mean over the evaluation set:
  win +1.0 / draw +0.1 / loss −1.0, plus margin shaping
  `0.25 × (own_remaining − enemy_remaining) / total`, minus anti-rush damping
  `0.2` if the match ends < 2 min. Exact weights live in `config.toml` and are
  injected, not hardcoded.
- Evaluation set per generation: 8 matches/genome — 4 self-play vs sampled
  population, 2 vs champion, 2 vs ghosts — with both spawn sides played.
- Every match run is seeded and **reproducible**; seeds + genome ids are logged
  per match (see §5).

## 3. Champion gating (the gauntlet)

- A generation winner is a *challenger*, not yet champion. Promotion requires:
  - ≥ 55% over 40 matches vs the reigning champion (20 seeds × both sides);
  - ≥ 50% aggregate over 20 matches vs 4 sampled historical champions.
- The champion genome is immutable until dethroned. On promotion the old
  champion moves to the Museum, lineage is updated, Elo recalculated, and a
  change report is generated.
- The gauntlet protocol is a pure function of (challenger, champion set, seeds,
  match executor); the result must be deterministic given those inputs.

## 4. Ghosts

- A ghost replays the *human side* of a recorded match: a frozen policy
  (deterministic function of `(tick, fog_view) -> commands`) reconstructed from
  the replay's command log. Same inputs ⇒ same commands (immutability).
- Ghost pool policy: keep last N=200 human matches + all matches that beat a
  champion + curator-pinned classics; recent ghosts weighted higher. Tunable,
  injected via config.

## 5. Reproducibility & lineage

- Lineage records ancestry (`parent_id`, generation, born_from) so any genome's
  descent is queryable.
- Elo (K=24, draws handled) applies to every genome with ≥ 10 league matches.
  Champion Elo history is the headline metric; a self-play floor check
  (≥ 70% vs the hard bot) raises a regression alarm if violated.
- **Every gauntlet/league match is logged with seeds and genome ids.** A
  promotion that cannot be reproduced from those logs is a determinism bug.

## 6. Guarantees to dependents

- `crucible-server` can call into this crate with its own match executor and
  storage callbacks; this crate never talks to SQLite or the network directly.
- Population/generation state is serializable so the server can checkpoint it
  atomically and resume a crashed generation cleanly.
