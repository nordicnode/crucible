# CRUCIBLE

A minimalist Command & Conquer-style RTS whose opponent is a
neuroevolution-trained AI that improves 24/7 — from self-play, from every human
match it plays, and from replays of human strategies ("ghosts").

See [`CRUCIBLE_development_plan.md`](CRUCIBLE_development_plan.md) for the full
design. This README tracks what is actually implemented.

## Status

- ✅ **M0 — Workspace & skeleton**
- ✅ **M1 — Deterministic sim core**
- ✅ **M2 — Scriptable match harness & scripted bots**
- ✅ **M3 — Server + live client**
- ✅ **M4 — AI commander + bootstrap curriculum**
- ✅ **M5 — Gauntlet, lineage, Elo, Museum API**
- ✅ **M6 — Trainer + dashboard/museum UI + auto-battle**
- ✅ **M7 — Ghost league**
- ✅ **M8 — Balance harness + tuning** (counter matrix committed as a 32-seed CI baseline; all three counters in the 35–65% band)
- ✅ **Champion & museum playable** — the live lobby now offers the reigning champion and any museum champion as opponents, not just scripted bots
- ✅ **Replay spectate** — any stored match can be watched step-by-step in the browser: the wasm shim re-runs the exact server sim from the input log (full state, no fog), with play/pause/speed/scrub
- ✅ **Bootstrap curriculum converges** — from random init, the staged curriculum (now ending in a combined easy+medium+hard gauntlet stage) reaches a genome that beats the hard bot 100% over 32 held-out maps; a cold-start **refuses to crown** unless that floor is met (CI-enforced in `crucible-evo/tests/curriculum.rs` + a structural gate in `bootstrap_cold`)

> **M8 note:** the counter matrix is in-band and directionally correct
> (tank > infantry 62%, artillery > tank 59%, infantry > artillery 56%).
> Bot pacing was retuned so match-length p50 sits in the 5–10 min target:
> rush-vs-turtle ~5.8 min (no more 2.5-min rush blowouts) and
> hard-vs-medium ~9.3 min (no more 14-min stalemates). The baseline is
> pinned at 32 seeds, and the turtle's finite turrets are what stop it from
> holding forever.

## Architecture

```
crucible/
  crates/
    crucible-sim/          PURE sim core (deterministic, no IO/threads/OS)
    crucible-ai/           AI commander (features, MLP, decision, scripted)
    crucible-evo/          Evolution strategy, lineage, ghosts, gauntlet, Elo
    crucible-server/       HTTP/WS + trainer + SQLite (the only impure crate)
    crucible-client-wasm/  wasm-bindgen shim for replay/spectate
  client/                  TypeScript + Vite + Canvas 2D (no framework)
```

Only `crucible-server` touches the network, filesystem, database, wall clock,
or threads. The client never implements game rules — live matches run
server-side.

## The determinism contract

- One injected seeded PRNG ([`rng`](crates/crucible-sim/src/rng.rs), a
  self-contained xoshiro256\*\*); no unseeded entropy anywhere.
- Fixed timestep: 10 ticks/second; one `Game::step()` per tick.
- Entities are iterated in ascending id order in every phase.
- Integer fixed-point math only — no floats, no transcendental functions, no
  platform-variable sqrt/pow in game-state math.
- `Game` is fully serde-serializable at any tick; replays are input logs
  (map seed + ordered commands), not state dumps.

Golden determinism tests hash the serialized state of a scripted match at fixed
ticks and fail if any byte changes. The *same* scenarios run on native
(`crucible-sim/tests/determinism.rs`) and under wasm
(`crucible-client-wasm/tests/wasm_parity.rs`) against one shared set of golden
constants (`crucible_sim::golden`), so native/wasm parity is enforced rather
than assumed. Two scenarios are pinned: one exercising economy/training/movement,
and one that sends both armies into a real battle, so combat resolution
(targeting, focus-fire, tank splash) is covered too.

CI (`.github/workflows/ci.yml`) enforces `cargo fmt --check`, clippy
(`-D warnings`), `cargo test --workspace`, the wasm32 build, the
wasm golden-parity test, and the client build + tests on every push and PR.

## How the AI learns

- **The commander, not the soldier.** The evolvable brain is a small MLP
  (~12k weights) that decides *strategy* on a 2-second command tick; unit
  micro (attack-move, harvesting, fleeing) is scripted. It sees only its own
  fog-of-war view and is capped at a human-plausible 120 APM.
- **Evolution strategy (μ+λ).** 64 genomes per generation; the top μ=16 are
  kept, λ=48 offspring are Gaussian-mutated (σ annealed 0.02→0.005, 10%
  macromutation). No crossover in v1, so lineage trees stay clean.
- **Bootstrap curriculum.** A cold-start population is shaped through six
  stages — economy (ore mined) → production (army value) → combat (vs idle)
  → scripted easy/medium/hard → a combined easy+medium+hard gauntlet stage —
  before entering the self-play league. The CI test proves it: **from random
  init, 14 generations reach a genome that beats the hard bot 100% over 32
  held-out maps** (schedule swept across master seeds; see
  `crucible-evo/tests/curriculum.rs`).

  Two honest limits worth knowing:
  - The bootstrap trains at a **2-minute match cap** (`bootstrap_match_timeout_ticks`),
    separate from the league's cap. At the full 6-minute league cap the same
    budget produces a rush specialist that *loses* to hard ~75% — so full-length
    skill is the self-play/ghost league's job, not the bootstrap's.
  - The plan's stronger bar (every champion beats **all three** scripted bots
    ≥ 90%) is **not yet enforceable**: the bootstrap champion is rush-tuned and
    its easy/medium rates are seed-dependent (easy 0–100%, medium 43–98% across
    seeds). That needs a stronger curriculum, not just an assertion — see
    `crates/crucible-evo/CONTRACT.md` §2.5.
- **Champion gating.** The generation winner only becomes the live champion
  if it wins a reproducible gauntlet: ≥55% vs the incumbent and ≥50% vs
  sampled historical champions. Every promotion is logged with seeds.
- **Ghost league.** Every human match is stored as a tiny input-log replay and
  replayed as a frozen "ghost" opponent during training, so the strategies
  that beat you become tomorrow's training data (post-upset focused cycles).
- **24/7 self-play.** The server's trainer loops generation → checkpoint →
  gauntlet → Elo, resuming from SQLite across restarts, all deterministically
  seeded.

## Building & testing

```bash
# Rust (native)
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets

# WASM target (sim + client shim)
rustup target add wasm32-unknown-unknown
cargo build -p crucible-sim --target wasm32-unknown-unknown
cargo build -p crucible-client-wasm --target wasm32-unknown-unknown

# Cross-target golden parity (native == wasm, run under node)
# The runner ships with wasm-bindgen-cli; pin to the Cargo.lock wasm-bindgen version.
cargo install wasm-bindgen-cli --version 0.2.127 --locked
cargo test -p crucible-client-wasm --target wasm32-unknown-unknown

# Client
cd client && npm install && npm run build && npm test
#   npm run build now also builds the wasm replay shim (scripts/build-wasm.sh),
#   which needs the wasm32-unknown-unknown target + wasm-bindgen-cli. For
#   `npm run dev` (vite), generate the bindings once first: npm run wasm

# Run the server (serves the built client on http://127.0.0.1:8787)
cargo run -p crucible-server

# Run the trainer (optional; CRUCIBLE_TRAINER=1 enables it)
#   CRUCIBLE_TRAINER_GENERATIONS=N  run a bounded fast-forward
#   CRUCIBLE_TRAINER_SMALL=1        use a small, fast population for demos
#   CRUCIBLE_TRAINER_BOOTSTRAP=1    run the staged curriculum on a cold start
CRUCIBLE_TRAINER=1 CRUCIBLE_TRAINER_SMALL=1 cargo run -p crucible-server
```

## Game model (v1)

One resource (ore), five buildings (HQ, Refinery, Barracks, Factory, Tech Lab,
Turret), four units (Harvester, Infantry, Tank, Artillery), 64×64 procedural
point-symmetric maps, fixed 100ms ticks, 15-minute cap. Win by destroying the
enemy HQ; timeout is scored by remaining value.

The complete action space (used by humans, the AI, ghosts, and tests alike,
through one validator): `PlaceBuilding`, `TrainUnit`, `MoveGroup`, `SetRally`,
`ChooseUpgrade`, `Sell` — gated by an APM budget (default 120/min).
