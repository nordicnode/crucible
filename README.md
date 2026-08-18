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
ticks and fail if any byte changes. The *same* scenario runs on native
(`crucible-sim/tests/determinism.rs`) and under wasm
(`crucible-client-wasm/tests/wasm_parity.rs`) against one shared set of golden
constants (`crucible_sim::golden`), so native/wasm parity is enforced rather
than assumed.

CI (`.github/workflows/ci.yml`) enforces `cargo fmt --check`, clippy
(`-D warnings`), `cargo test --workspace`, the wasm32 build, the
wasm golden-parity test, and the client build + tests on every push and PR.

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

# Run the server (serves the built client on http://127.0.0.1:8787)
cargo run -p crucible-server

# Run the trainer (optional; CRUCIBLE_TRAINER=1 enables it)
#   CRUCIBLE_TRAINER_GENERATIONS=N  run a bounded fast-forward
#   CRUCIBLE_TRAINER_SMALL=1        use a small, fast population for demos
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
