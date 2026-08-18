# CONTRACT — crucible-sim

The pure, deterministic simulation core. This crate is the single source of
truth for game rules; every other crate and the client treat its behavior as
law. **Status: implemented (M1).**

## 1. Purity boundary

`crucible-sim` MUST NOT:

- open files, sockets, or the wall clock;
- spawn threads or tasks;
- make OS calls or touch the environment;
- draw entropy from anywhere except the injected seeded PRNG (`rng::Rng`);
- depend on any crate that requires OS/JS bindings (it must compile for
  `wasm32-unknown-unknown` with no `getrandom`, no `std::time`, no `std::thread`).

The crate depends only on `serde`/`serde_json` (serialization) plus its own
modules. Any future system-resource need must be pushed to `crucible-server`
and injected.

## 2. Determinism contract (this is the whole point)

1. **Fixed timestep.** `Game::step()` advances exactly one tick = 100 ms.
   `TICKS_PER_SEC = 10`. There is no variable-duration step. The command tick
   is every 20 sim ticks (2 s): `COMMAND_TICK = 20`.
2. **One PRNG.** All randomness flows through `rng::Rng`, a self-contained
   xoshiro256\*\* seeded from a `u64`. No unseeded entropy exists in the sim.
   A pinned known-sequence test guards the exact stream.
3. **Integer math only.** Positions/quantities are fixed-point integers
   (`fixed.rs`, 1 tile = 256 fix units). No `f32`/`f64` in game-state math; no
   `sin`/`cos`/`powf`/`sqrt` (distance uses `isqrt` + squared comparisons).
   A `HashMap`/`HashSet` must never influence a sim outcome; entity storage is
   `Vec` and pathfinding uses a tie-broken `BinaryHeap`.
4. **Entity order is spec.** Entities are assigned ids in ascending creation
   order from a single allocator. Every phase (economy, production, combat,
   turret fire, fog) iterates entities in **ascending id order**. Death sweeps
   use `retain`, preserving relative order.
5. **Byte-identical cross-target.** Identical seed + command log ⇒ identical
   serialized state on `x86_64-unknown-linux-gnu` and `wasm32-unknown-unknown`.
   Golden tests hash `serialize::snapshot_bytes` at fixed ticks and fail on any
   byte change.

## 3. Fixed tick order

`Game::step()` runs, in this exact order:

1. `tick += 1`; APM budgets refill.
2. Cooldowns decrement (units, then buildings).
3. `economy_phase` — refinery trickle, harvester mining/hauling/dropoff/flee.
4. `production_phase` — queues progress; completed units spawn (id order).
5. `combat_phase` — per combat unit: acquire target, move, fire (id order).
6. `turret_phase` — turrets fire (id order).
7. `sweep_dead` — dead entities removed.
8. `fog_phase` — visibility recomputed; last-seen memory updated.
9. `check_win` — HQ destroyed, or timeout by remaining value.

Reordering these changes determinism and requires a golden-hash update.

## 4. Command & validation contract

- The complete action space is `orders::Command`: `PlaceBuilding`,
  `TrainUnit`, `MoveGroup`, `SetRally`, `ChooseUpgrade`, `Sell`.
- **One validator.** `Game::validate_command` is the only validation path;
  `apply_commands` validates, charges the APM budget, then executes. Humans,
  the AI, ghosts, and tests all go through it. No bypass exists.
- The APM cap (default 120 commands/min) is enforced *inside* the sim as a
  token bucket in `ApmBudget`; over-budget commands return `RateLimited` and
  increment `dropped_commands`.
- Economy rules: train cost is charged at queue time; sell refunds 50%;
  building placement requires the build radius around own HQ/refinery, a
  passable, ore-free, unoccupied tile; artillery production requires a Tech
  Lab; Tech Lab placement requires a Factory; the upgrade is chosen once.

## 5. Fog-of-war contract

- `fog::FogView` is the *only* observation object exposed to a player (and the
  only input the AI may read). It contains currently-visible tiles,
  remembered enemy units/buildings with `last_seen` ticks, and known ore tiles.
  It cannot contain a live hidden entity.
- `Game::fog_phase` runs each tick and maintains `FogMemory` in serialized
  state; remembered positions decay (dropped after 60 s unseen).

## 6. Serialization & replay contract

- `Game` is `Serialize`/`Deserialize` and byte-stable at any tick (field order
  is definition order).
- A replay is an **input log**: `{version, map_seed, config, commands[],
  result?}` (`serialize::Replay`), not a state dump. `FORMAT_VERSION = 1`.
  Version envelopes exist from day one; old replays must stay re-runnable.

## 7. Guarantees to dependents

`crucible-ai`, `crucible-evo`, `crucible-server`, and `crucible-client-wasm`
may rely on: the determinism guarantees above; the public types re-exported
from `lib.rs`; and `Map::generate(seed)` producing a point-symmetric,
fully-connected 64×64 map (mirror `(x,y) -> (63-x,63-y)`), with both HQs
mutually reachable and every ore tile reachable from both HQs.
