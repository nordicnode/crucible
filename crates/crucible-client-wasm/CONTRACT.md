# CONTRACT — crucible-client-wasm

The `wasm-bindgen` shim exposing `crucible-sim` to the browser. **Status: M6
— replay execution (`replay_result`, `replay_snapshot_json`) implemented; a
wasm-bindgen-test/node runner for cross-target golden parity is still pending.**

## 1. Purpose & scope

This crate exists **only** so the browser can run the *same* deterministic sim
for local replay and spectate (replays, auto-battles). It is a thin
passthrough over `crucible-sim`, not a second implementation.

## 2. Hard rules

- **Never used for live matches.** Live matches are server-authoritative
  (`crucible-server/CONTRACT.md` §1). The wasm sim exists for local replay
  verification and spectating only — never for trust.
- **No game rules here.** It may allocate a `Game`/`Map`, apply command logs,
  step ticks, and serialize snapshots, but must not modify or duplicate game
  logic. Any behavior difference from native `crucible-sim` is a bug.
- **Same determinism.** Byte-identical to native: same seed + command log ⇒
  same serialized state. Golden tests that run natively must also run under
  wasm (wasm-bindgen-test/node) and produce identical hashes.

## 3. API surface

- Exposes the minimum needed by `client/src/wasm/`: construct a game from a
  replay (seed + config + commands), step to a tick, and return a snapshot as
  JSON. `wasm_bindgen` bindings only; no DOM, no JS imports beyond the glue.
- Versioned entry points (e.g., `replay_snapshot_json(replay_json, tick)`),
  mirroring the `crucible-sim` replay format version.

## 4. Build contract

- Compiles to `wasm32-unknown-unknown` with zero `getrandom`/OS/thread
  dependencies (inherited from `crucible-sim`). Bundled size target: the
  client (JS + WASM) stays under 1 MB total.
