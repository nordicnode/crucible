# CRUCIBLE — Development Plan

> A minimalist Command & Conquer-style RTS where the opponent is a neuroevolution-trained AI that learns 24/7 — from self-play, from every human match it plays, and from replays of human strategies ("ghosts") — with champion lineages gated by deterministic gauntlets so its skill only ever climbs.
>
> **Target:** local server (Linux dev machine) serving a browser client; deployable to a VPS later with zero architectural changes.
> **Primary constraint:** no LLMs, no external APIs, no GPU training. The AI is a small MLP evolved with an evolution strategy; training is just running the deterministic sim headless.

---

## 1. Project Overview

### 1.1 Elevator pitch
A stripped-down RTS — one resource, five buildings, five units, small procedural maps, 5–10 minute matches, flat-color vector rendering. The AI plays the **commander**, not the soldiers: a compact neural network makes strategic decisions on a slow command tick (build, train, expand, attack, defend) while individual units run scripted local rules. The server trains the AI continuously in headless self-play, folds every human match into the training pool as replayable ghosts, and promotes new champions only when they beat the reigning one. It gets better while you sleep, and a dashboard proves it.

### 1.2 Design pillars
1. **The commander, not the soldier** — the evolvable brain is small (~5–10k params) and decides strategy; micro is scripted. This is what makes RTS neuroevolution converge on commodity hardware.
2. **Fair by construction** — the AI gets the same fog of war and a human-plausible command-rate cap. It must win with better strategy, not superhuman micro. This makes its improvements legible and worth studying.
3. **Improvement is content** — Elo graphs, generational change reports, ancestor boss fights, AI-vs-ancestor spectating. The player must be able to *see* it learning.
4. **Determinism everywhere** — one seeded PRNG, fixed timestep, serializable state. Replays are input logs (a few KB), ghosts are replayed decisions, gauntlets are reproducible.
5. **Every human game is selection pressure** — beat the champion with a cheese strategy and the lineage evolves against your ghost within a day.

### 1.3 Non-goals (do not build)
- No network calls to external services at runtime (no telemetry, no CDNs, no package fetches post-build).
- No LLM/API-driven behavior anywhere.
- No GPU training, no backpropagation infrastructure (no TensorFlow/PyTorch — forward-pass MLP + evolution strategy only).
- No unit-level learned micro, no neural pathfinding.
- No graphical fidelity: flat-color canvas/WebGL-free rendering; no art assets in v1.
- No account system (v1 is single-human vs AI on localhost; player identity is a config string).
- No real-money anything, no ads, no analytics.

### 1.4 Honest scope note
This will not produce AlphaStar. It will produce an opponent with visible personality, genuine strategic adaptation to the player's habits, and a measurable skill climb over weeks of server uptime. That is the goal; do not chase superhuman play.

---

## 2. Tech Stack & Constraints

| Decision | Choice | Rationale |
|---|---|---|
| Sim core language | **Rust** (single crate, dual target) | Native build for server headless training (max matches/sec); WASM build for browser client; one codebase guarantees identical behavior |
| Determinism | Fixed-point or f64-only sim math, seeded PRNG | Replays, ghosts, gauntlets all depend on it |
| PRNG | `rand_chacha` (ChaCha8) or `rand_pcg`, seeded from u64 | Reproducible across native and WASM builds — verify with golden tests |
| Server | Rust (axum or actix-web) + tokio | Same language as sim core; serves static client, WebSocket matches, training orchestration, SQLite |
| Persistence | SQLite (via `sqlx` or `rusqlite`) | Genomes, lineage, replays, Elo history, training stats; single file, zero ops |
| Client | TypeScript + Vite, Canvas 2D | Thin: renderer + input + dashboard UI; all game logic runs in the WASM sim |
| AI network | Hand-rolled MLP on flat arrays (`Vec<f32>`) | ~150 lines; no ML framework needed since we never backpropagate |
| Training | Native Rust, rayon for parallel match evaluation across cores | Headless sim instances; configurable CPU budget |
| Protocol | JSON over WebSocket for v1 (binary optional later) | Debuggability beats bandwidth at this scale |
| Testing | `cargo test` (unit + golden determinism), vitest for client UI logic | Golden-file determinism tests are the foundation |

### 2.1 Hard constraints for all code
- The sim crate (`crucible-sim`) must compile for both `x86_64-unknown-linux-gnu` and `wasm32-unknown-unknown` with **identical behavior**. No OS calls, no wall-clock time, no threads inside the sim.
- All randomness flows through one injected seeded PRNG. No unseeded entropy anywhere in the sim or AI. Clippy lint + code review rule.
- Fixed timestep: sim advances at 10 ticks/second of game time, decoupled from render and network framerates.
- Game state must be serializable (serde) at any tick. Replays = initial state + ordered command list.
- **No floating-point platform traps:** avoid transcendental functions with platform-variable results in sim hot paths (no `sin`/`cos`/`powf` on game-state math; precomputed tables or integer approximations where needed). Document any exceptions.
- The AI commander may only observe what a human could observe (fog of war) and may only issue commands on the command tick (every 20 sim ticks = 2s game time) with a per-match command budget (APM cap). Enforced *inside* the sim's command validator, not by convention.

---

## 3. Architecture

### 3.1 Workspace layout (Rust workspace + TS client)

```
crucible/
  Cargo.toml                 # workspace
  crates/
    crucible-sim/            # PURE sim core — no OS, no IO, no threads
      src/
        lib.rs
        rng.rs               # seeded PRNG wrapper
        map.rs               # procedural map gen from seed
        entity.rs            # units, buildings, components
        orders.rs            # command types + validation (incl. APM cap)
        combat.rs            # deterministic combat resolution
        economy.rs           # ore fields, harvesting, income
        fog.rs               # fog-of-war views per player
        tick.rs              # the fixed-step advance
        game.rs              # match orchestration, win check
        serialize.rs         # snapshot + replay (input log) formats
    crucible-ai/             # PURE — depends only on crucible-sim
      src/
        lib.rs
        features.rs          # fogged game state -> input vector (~120 floats)
        network.rs           # MLP: init, forward, mutate (flat Vec<f32>)
        decision.rs          # network outputs -> validated commands
        scripted.rs          # scripted opponents (for bootstrap/tests/baselines)
    crucible-evo/            # PURE training logic
      src/
        population.rs        # (mu+lambda) ES, selection, annealed sigma
        fitness.rs           # match evaluation, shaped fitness
        lineage.rs           # ancestry records, champion history
        ghost.rs             # replay-as-opponent wrapper
        gauntlet.rs          # champion promotion protocol
        league.rs            # Elo tracking, historical champions
    crucible-server/         # binary: orchestration + IO (the ONLY impure crate)
      src/
        main.rs
        http.rs              # static files + REST (dashboard data, replays)
        ws.rs                # live match protocol (server-authoritative)
        trainer.rs           # 24/7 training loop, rayon pool, CPU budget
        store.rs             # SQLite access, migrations
        report.rs            # change reports, "while you were away"
    crucible-client-wasm/    # wasm-bindgen shim exposing sim to JS
  client/                    # TypeScript + Vite
    src/
      main.ts                # entry, screen router
      net.ts                 # WebSocket client, command batching
      render/
        renderer.ts          # Canvas 2D, flat colors, camera, selection
        minimap.ts
      ui/
        screens/             # lobby, match, results, dashboard, museum
        panels/              # build menu, resource bar, event log
      wasm/                  # loader for crucible-client-wasm (spectate/replay)
  data/
    .gitignore               # SQLite db lives here (runtime)
  docs/
```

### 3.2 Crate dependency rules
- `crucible-sim` depends on nothing but serde/rand crates. It is the single source of truth for game rules.
- `crucible-ai` and `crucible-evo` are pure: no IO. Server injects storage and scheduling.
- Only `crucible-server` touches the network, filesystem, database, wall clock, and threads.
- Client **never** implements game rules. Live matches run server-side; client sends commands and renders state diffs. Spectate/replay mode runs the WASM sim locally from an input log.

### 3.3 Process model

```
┌─ crucible-server ────────────────────────────────┐
│  HTTP/WS (axum)        Trainer (tokio task)      │
│  ├─ live matches       ├─ rayon pool: headless   │
│  ├─ dashboard API      │  matches (self-play,    │
│  └─ static client      │  ghosts, gauntlets)     │
│                        ├─ champion gating        │
│  SQLite                └─ lineage + Elo updates  │
└──────────────────────────────────────────────────┘
         ▲ WS (commands down, state diffs down)
         │
   Browser client (TS + Canvas, WASM for replays)
```

- **Live match**: server runs the sim at fixed timestep; client sends player commands; server validates (same validator the AI uses, incl. APM cap) and broadcasts fogged state diffs. Client renders.
- **Trainer**: continuous loop. Prioritizes in order: (1) pending gauntlets (champion challenges), (2) ghost-league matches vs recent human replays, (3) self-play generations across random seeds. CPU budget configurable (cores, duty cycle); yields to live matches instantly.

---

## 4. Game Design (sim contract)

### 4.1 Match structure
- Procedural map from u64 seed: 64×64 tile grid (v1), 2 HQ spawns, ore fields (main base + 2–4 expansion sites), terrain features creating 1–3 chokepoints. Symmetric fairness: mirror or rotationally balanced resource placement (map gen guarantees both players equal-quality starts — assert in tests).
- Win: destroy enemy HQ. Timeout at 15 min game time → higher remaining (buildings + units + banked ore) value wins.
- Match length target: 5–10 minutes.

### 4.2 Economy
- One resource: **ore**. Harvesters carry ore from field to refinery. Fields deplete; expansions matter in long games.
- Starting bank: 500 ore. Costs (initial balance): refinery 300, barracks 150, factory 250, tech lab 200, turret 100; infantry 50, harvester 100, tank 150, artillery 200.

### 4.3 Units & buildings (v1)

| Unit | Cost | HP | Damage | Speed | Role | Countered by |
|---|---|---|---|---|---|---|
| Harvester | 100 | high | none | slow | economy; defenseless | everything |
| Infantry | 50 | low | low | fast | swarm, scout, anti-artillery | tank splash |
| Tank | 150 | med | med (splash) | med | backbone | artillery (outranged) |
| Artillery | 200 | low | high (long range, min range) | slow | siege | infantry (inside min range) |
| Turret (building) | 100 | med | med | — | static defense | artillery; economic containment |

Buildings: HQ (produces nothing; losing it loses the game), Refinery (drop-off + slight income trickle), Barracks (infantry), Factory (harvester, tank), Tech Lab (unlocks artillery + one global upgrade tier: +15% damage OR +15% HP, chosen per match).

### 4.4 Unit behavior (scripted local rules — NOT learned)
- Attack-move: engage enemies in aggro radius, focus lowest-HP in range, artillery holds at max range, units retreat at <20% HP only if ordered stance is "cautious".
- Harvesters: auto-loop field→refinery; flee when attacked (return when safe).
- No collision-avoidance fanciness: simple separation steering, deterministic tie-breaking by entity id.

### 4.5 Player commands (the complete action space)
1. `PlaceBuilding(type, tile)` — build radius around HQ/refineries.
2. `TrainUnit(building, type)` — queued production.
3. `MoveGroup(units, waypoint, stance)` — stance ∈ {aggressive, cautious, hold}.
4. `SetRally(building, waypoint)`.
5. `ChooseUpgrade(lab, damage|hp)`.
6. `Sell(building)` (50% refund) — enables desperate comebacks.

This deliberately small command set is what the AI outputs too — same validator for both sides.

---

## 5. The AI Commander (design contract)

### 5.1 Interface: the command tick
- Sim tick = 100ms. **Command tick = every 20 sim ticks (2s).**
- On each command tick, the AI receives a feature vector computed from its *fogged* view, runs one forward pass, and may issue up to **K commands per minute** (APM cap, default K=120 — human-plausible). Excess commands are dropped by the validator with a counter exposed for debugging.
- Between command ticks, the world runs on scripted unit behavior. The AI never micros individual units beyond group orders — same as the human's effective control.

### 5.2 Feature vector (~120 floats, all fog-of-war legal)
- Economy: banked ore (normalized), income rate, harvester count, ore remaining at known fields, expansion count.
- Army: own composition counts; *observed* enemy composition (last-seen, decayed); army value ratio estimate.
- Map: known enemy building positions (fuzzed to sector grid, staleness-decayed), chokepoint ownership estimates, unexplored fraction.
- Tempo: game time, own HQ HP, last-known enemy HQ HP, recent damage dealt/taken (rolling windows).
- Self context: current production queues, idle buildings count, tech state, chosen upgrade.
- History embedding: mean of last N command-tick hidden states (simple recurrent trick — carry hidden activations between ticks; keeps the network feed-forward while giving it memory).

### 5.3 Network & genome
- MLP: ~120 inputs → 2 hidden layers (48, 48, tanh) → output heads:
  - **Build head**: scores over {building types × candidate tiles near base} (tiles discretized to a small candidate set from map analysis).
  - **Train head**: scores over {unit types} + queue length modulation.
  - **Army head**: {attack, defend, expand, scout, mass} stance + target sector (map divided into 8×8 sectors → 64-way sector score).
  - **Tech head**: {none, damage, hp}.
- Outputs are *scores*; the decision layer (`decision.rs`) converts them to concrete valid commands via masked argmax + thresholds (illegal moves masked out). Deterministic given genome + state.
- Genome = flat `Vec<f32>` of all weights/biases (~10–12k params, ~45 KB JSON). Versioned schema.

### 5.4 Evolution strategy ((μ+λ) ES)
- Population 64 genomes. μ = top 16 retained; λ = 48 offspring via Gaussian mutation (σ annealed 0.02→0.005 by generation) + 10% macromutation rate (one layer re-perturbed at 3σ). No crossover in v1 (keep lineage trees simple; evaluate crossover in v2).
- Fitness per genome = mean over evaluation set:
  - win +1.0 / draw +0.1 / loss −1.0
  - + 0.25 × (own remaining value − enemy remaining value) / total value (margin shaping)
  - − 0.2 if match ends < 2 min (anti-degenerate-rush damping, tunable)
- Evaluation set per generation: 8 matches per genome — 4 self-play (vs sampled population members), 2 vs current champion, 2 vs ghosts (if available). All on random seeds from the generation's seed list. Mirror matches: each pairing plays both spawn sides.
- Elitism: champion genome is immutable until dethroned by the gauntlet (§5.5).

### 5.5 Champion gating (the gauntlet)
A challenger genome (generation winner) must win a promotion gauntlet before becoming the live champion:
- 40 matches vs reigning champion (20 seeds × both sides) → must win ≥ 55%.
- 20 matches vs 4 sampled historical champions → must win ≥ 50% aggregate (prevents degenerate "beats only current champ" overfit).
- All gauntlet matches logged with seeds; fully reproducible from DB records.
- On promotion: old champion moves to the Museum (historical ladder), lineage record updated, Elo recalculated, change report generated.

### 5.6 Ghost league (human strategies as training data)
- Every human match stores: map seed, both sides' full command logs, final result. (A replay is tiny — input log only.)
- A **ghost** replays the human side of a recorded match against training genomes. Ghosts are frozen policies (they don't adapt), which is fine: they inject human novelty into the selection landscape.
- Ghost pool policy: keep last N=200 human matches + all matches that beat a champion + curator-pinned "classic" ghosts. Weight recent ghosts higher in sampling.
- After any human beats the live champion: trainer prioritizes a focused ghost-league cycle against that replay within the next training window. (This is the "your cheese dies within a day" feature.)

### 5.7 Bootstrap curriculum (cold start)
A randomly-initialized population can't even build a refinery. Bootstrap in stages (each stage's fitness gate must be met before advancing):
1. **Economy shaping**: fitness = ore harvested in 5 min (vs no opponent).
2. **Production shaping**: fitness = army value built by minute 6.
3. **Combat shaping**: vs idle scripted enemy — fitness = damage dealt + win.
4. **Real matches**: vs scripted bots (easy → medium → hard) — standard fitness.
5. **Full league**: self-play + ghosts + champion gate.
Scripted bots (easy: passive turtle; medium: periodic attack waves; hard: competent expand-and-push) are also permanent regression baselines: every promoted champion must beat all three ≥ 90%.

### 5.8 Elo & measurement
- Every genome that completes ≥ 10 league matches gets an Elo (standard Elo, K=24, draws handled).
- Champion Elo history is the dashboard's headline graph. Compute a **self-play Elo floor check**: champion must maintain ≥ 70% vs bootstrap hard bot forever (regression alarm if it dips — training bug detector).

---

## 6. Visibility Features (improvement as content)

### 6.1 Dashboard (web UI, served by the same server)
- Champion card: genome id, generation, Elo, reign length, parent lineage.
- Elo-over-time graph (all champions, log-x generations).
- Population stats: current generation, fitness spread, diversity metric (mean pairwise weight distance).
- Training throughput: matches/hour, generations/day, ghost pool size.
- **Change reports** (`report.rs`): on each promotion, diff behavioral fingerprints between old and new champion — fingerprint = aggregate stats over 100 evaluation matches (first-attack time, expansion count, composition distribution, average game length, economy/army spend ratio). Human-readable: "gen 1,240 attacks ~40s earlier, double-expands 3× more often, abandoned infantry spam."

### 6.2 The Museum
- Every historical champion listed with era stats and change report.
- Each is **playable as a boss** (human vs any historical champion).
- **Auto-battles**: watch current champion vs any ancestor (server runs it, client spectates via replay stream). Watching gen 2,000 dismantle gen 500 is the visceral proof of improvement.
- Era naming: simple classifier over fingerprints names playstyle eras (e.g., "The Turtle Dynasty", "The Rush Years").

### 6.3 "While you were away"
- Server accumulates events since the player's last session: generations run, promotions, gauntlet upsets, new dominant strategy detected, your ghost's record in the league. Shown on dashboard landing.

---

## 7. Server & Protocol

### 7.1 REST endpoints (dashboard/data)
- `GET /api/champion` — current champion + stats
- `GET /api/elo-history`, `GET /api/lineage?genome_id=`, `GET /api/museum`
- `GET /api/replays?limit=`, `GET /api/replay/:id` (input log + metadata)
- `GET /api/status` — trainer state, CPU budget, uptime, away-report
- `POST /api/match` — request new match (returns WS ticket)
- No auth in v1 (localhost). Auth hook left in router config for VPS future.

### 7.2 WebSocket live-match protocol
- Client → server: `JoinMatch { opponent: Champion|Museum(id)|Sandbox }`, then `Commands { tick, cmds: [...] }` batched per command tick.
- Server → client: `MatchStart { map_seed, player_side }`, then per-tick `StateDiff { tick, entities: [...], events: [...] }` (fogged; JSON v1), `MatchEnd { result, replay_id }`.
- Server-authoritative: client commands validated identically to AI commands. Invalid → rejected with reason (client displays).
- Disconnect: match pauses (v1) or converts to ghost-forfeits (v2).

### 7.3 Trainer loop (pseudocode contract)
```
loop {
  if live_match_active { yield_cpu() }
  if let Some(challenger) = pending_gauntlet.pop() { run_gauntlet(challenger) }
  else if ghost_cycle_due() { run_ghost_league_batch() }
  else { run_selfplay_generation() }
  checkpoint_to_sqlite()  // population + lineage + stats, atomic-ish
}
```
- CPU budget: config (cores, duty cycle %). Default: all cores at 60% duty when idle, 0% during live matches on ≤4-core machines.

### 7.4 Database schema (SQLite, migrated)
- `genomes(id, generation, parent_id, weights BLOB, born_from, created_at)`
- `champions(genome_id, crowned_at, dethroned_at, gauntlet_record JSON)`
- `matches(id, seed, p1_type, p1_id, p2_type, p2_id, result, duration_ticks, replay BLOB, created_at)`
- `elo_history(genome_id, at, elo)`
- `training_stats(at, generation, matches_run, pop_fitness_mean, diversity)`
- `events(at, kind, payload JSON)` — feeds the away report
- Schema versioned; `store.rs` runs migrations at boot.

---

## 8. Client (TypeScript)

- **Renderer**: Canvas 2D, flat colors, entities as shapes (squares=buildings, circles=infantry, triangles=tanks, diamonds=artillery). Camera pan/zoom, box-select, right-click orders, control groups. Fog as dimming. Minimap with sector grid (same 8×8 sectors the AI uses — nice symmetry, helps players read AI target choices).
- **Screens**: lobby (choose opponent: champion / museum / sandbox scripted), match HUD (ore, production queues, event log), results screen (with "watch replay" and "this match is now ghost #N" note), dashboard (§6), museum (§6.2).
- **WASM usage**: replays and auto-battles run client-side from input logs via `crucible-client-wasm` — server just serves the log. Live matches never use client-side sim (no cheating surface, no sync bugs).
- Keyboard-first bindings; full mouse play. Target: playable by any RTS player in < 1 minute.

---

## 9. Milestones

Each milestone is independently testable and runnable. **Do not skip ahead — determinism tests gate everything.**

### M0 — Workspace & skeleton (0.5 day)
- Cargo workspace (4 crates), Vite client, CI (fmt/clippy/test/build wasm), server serving "hello" + static client.
- **Acceptance:** `cargo test` green, `trunk`/`wasm-pack` (or wasm-bindgen-cli) builds the WASM shim, server serves client on localhost.

### M1 — Deterministic sim core (2–3 days) ⚠️ FOUNDATION
- `rng`, `map` gen (with fairness invariant), entities, economy, movement, combat, fog, `tick`, win check, serde snapshots.
- **Acceptance:**
  - Golden tests: same seed + same command log → byte-identical serialized state at ticks 100/1k/10k/50k, on **both native and WASM builds** (cross-target determinism test committed).
  - Map fairness test: 10k seeds, resource-distance symmetry within tolerance for both spawns.

### M2 — Scriptable match harness (1–2 days)
- Headless runner: load scenario, inject command scripts, assert outcomes. Scripted bots (easy/medium/hard) in `crucible-ai/scripted.rs`.
- **Acceptance:** scenario tests — harvester economy reaches X ore by minute 5; tanks beat equal-cost infantry; artillery outranges turrets; rush beats turtle if un-scouted; hard bot beats medium ≥ 80%.

### M3 — Server + live client (3–4 days)
- axum server, WS protocol, Canvas client (render, input, HUD), human vs scripted bot end-to-end.
- **Acceptance:** full playable match in browser vs hard bot; command validation rejects illegal/APM-exceeding input; match result + replay stored in SQLite; replay re-runs byte-identically via WASM in browser.

### M4 — AI commander + bootstrap curriculum (3–4 days) 🧠
- `features`, `network`, `decision`, ES population, staged bootstrap fitness, trainer loop (self-play only), champion crowning v1 (no gauntlet yet).
- **Acceptance:**
  - Convergence test: from random init, lineage passes all 5 curriculum stages; beats hard bot ≥ 90% within a bounded generation budget on the dev machine (record actual numbers in docs).
  - Feature legality test: fuzz — AI features provably derive only from fogged state (test asserts hidden entities contribute zero to feature deltas).

### M5 — Gauntlet, lineage, Elo, Museum API (2 days)
- Champion gating protocol, historical champions, Elo tracking, all REST endpoints, change-report generator.
- **Acceptance:** scripted experiment — inject a genome known to beat the champion (trained offline in a test), run gauntlet, assert promotion occurs only when win-rate thresholds met; museum lists dethroned champions; Elo history monotonic-ish across promotions (allow small noise).

### M6 — Dashboard + museum UI + spectate (2–3 days)
- Dashboard graphs (Elo over time, training stats), museum browser, auto-battle spectate via WASM replay streaming, away report.
- **Acceptance:** kill server for a simulated "day" (fast-forward trainer), restart, dashboard shows overnight progress; any two champions can auto-battle and the result matches a headless run of the same seeds.

### M7 — Ghost league (1–2 days)
- Replay-to-ghost wrapper, ghost pool policy, sampling in training, post-upset focused cycles.
- **Acceptance:** record a human beating champion via a scripted cheese strategy; assert trainer prioritizes that ghost and within a bounded generation budget the lineage's win rate vs that ghost exceeds threshold (e.g., >70%).

### M8 — Balance harness + tuning (ongoing)
- Batch headless tool: N matches per matchup across seed sets → win-rate tables (unit counters, bot tiers, champion vs historicals). Committed baseline tables; CI check on sim-affecting changes.
- **Acceptance:** counter matrix within target bands (no unit >65% / <35% in its counter matchup at equal cost); match length p50 within 5–10 min.

**Critical path:** M1 → M2 → M3 → M4. M5–M7 can reorder after M4.

---

## 10. Testing Strategy

| Layer | Method |
|---|---|
| Determinism | Golden tests (seeds × tick counts), native **and** WASM, byte-identical snapshots |
| Map gen | Fairness invariants over 10k seeds; reachability (both HQs path-connected to all ore) |
| Combat/economy | Scripted scenario tests (M2 harness) |
| Command validation | Fuzz: random command streams must never crash sim; illegal always rejected |
| AI features | Fog-legality fuzz (hidden entities → zero feature delta) |
| Evolution | Curriculum convergence test (bounded budget); gauntlet protocol test with rigged genomes |
| Ghosts/replays | Record → replay → byte-identical state; ghost immutability (same inputs → same commands) |
| Server | Integration tests: WS match lifecycle, REST schema, SQLite migrations from fixture DBs |
| Client | Vitest for UI logic (state diff application, command batching); manual playtest checklist |
| Regression | Champion must beat all scripted bots ≥ 90% forever; CI alarm on sim changes |

---

## 11. Implementation Notes for the Coding Agent

1. **Determinism is sacred.** All randomness via the injected PRNG. No wall clock, no HashMap iteration order leaking into sim outcomes (use BTreeMap or index-based storage for entity iteration — entity order must be insertion-stable and id-deterministic). No platform-variable float functions in game-state math.
2. **Entity iteration order is part of the spec.** Process entities in ascending id order everywhere (combat, movement, harvesting). Document it; test it.
3. **The sim crate is pure.** No IO, no threads, no OS. If you need a system resource, you're in the wrong crate — push it to the server and inject the result.
4. **One command validator.** Humans, AI, ghosts, and tests all issue commands through the same validation path. The APM cap applies to the AI *in the sim*, not as an honor-system wrapper.
5. **Fog legality is enforced by construction.** `features.rs` receives a `FogView` struct that literally cannot contain hidden entities — not a full state with a "please don't peek" comment.
6. **Genome and replay formats are versioned from day one** (`{version: 1, ...}` envelopes). Old ghosts and old champions must stay replayable; migrations are part of `store.rs`.
7. **Start ugly.** Server-rendered JSON dashboard with `<pre>` and SVG sparklines before any chart library. Client renders squares and circles before any polish. Pretty is a trap before the training loop is proven.
8. **Checkpoint atomically.** Trainer writes population to SQLite in a transaction; a crash mid-generation must resume cleanly (write generation N+1 only when complete).
9. **Log every gauntlet match.** Seeds + genome ids per match, stored in DB. If a promotion can't be reproduced from logs, that's a bug report against determinism, not a shrug.
10. **Keep the bundle lean.** Client deps: Vite + TS only (no framework for v1; screens are simple enough). Total client payload target < 1 MB including WASM.
11. **Config over constants.** CPU budget, APM cap, population size, gauntlet thresholds live in `config.toml` with sane defaults; tests override via builder.
12. **Commit cadence:** one commit per acceptance criterion; milestone tags `m1`…`m8`.

---

## 12. Open Questions (resolve at the noted milestones)

- [ ] **Crossover or mutation-only?** (M4) Mutation-only keeps lineage trees clean; crossover may speed convergence. A/B with the harness once it exists.
- [ ] **Recurrent trick sufficiency.** (M4) Hidden-state carry is cheap but shallow memory. If scouting/adaptation looks weak, consider adding the last K command ticks' features as extra inputs before changing architecture.
- [ ] **Build placement granularity.** (M1/M4) Candidate-tile approach (small discrete set) vs. learned offsets. Candidate set is simpler and masks cleanly; revisit if the AI's base layouts are pathologically bad.
- [ ] **Draw handling in fitness.** (M4) Current shaping may reward turtling; watch match-length distribution in M8 and adjust.
- [ ] **Ghost pool size/weighting.** (M7) N=200 with recency weighting is a guess; tune after observing training throughput.
- [ ] **Client-side sim for live matches?** (M3) v1 is server-authoritative only. If localhost latency is ever noticeable (it won't be), revisit — never for trust reasons, only feel.
- [ ] **Multiple concurrent human players.** (post-M8) Server design already supports N humans (each match is independent; trainer just gets more ghosts). UI is single-player v1; lobby/multiplayer is a later milestone, not an architectural change.

---

## 13. Definition of Done (v1.0)

- [ ] Human can play a full match in browser vs the live champion on localhost; server-authoritative, replays stored.
- [ ] Trainer runs 24/7 within CPU budget; survives restarts with zero lost state.
- [ ] Bootstrap curriculum converges from random init to beating the hard scripted bot ≥ 90%, reproducibly.
- [ ] Champion promotions only via gauntlet; full lineage + Elo history queryable; museum playable.
- [ ] Dashboard shows Elo-over-time, training stats, change reports, and an accurate "while you were away".
- [ ] Ghost league demonstrably adapts: scripted-upset test (M7 acceptance) passes.
- [ ] Determinism golden tests green on native and WASM in CI; balance tables within target bands.
- [ ] README with architecture diagram, "how the AI learns", and instructions: one command to run server, open browser, play.

---

## 14. Future (post-v1, do not build now)

- Deploy to VPS → multi-human ghost pool (the "learns from every player in the world" endgame).
- Genome import/export + community PR gauntlet (the CI-driven global champion idea, portable from earlier design).
- Map mutators ("Laws") as per-match rule variations for extra generalization pressure.
- Cooperative 2v1 vs an evolved champion; AI allies.
- Binary WS protocol if bandwidth ever matters.
