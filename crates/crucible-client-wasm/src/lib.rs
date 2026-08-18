//! wasm-bindgen shim exposing the deterministic sim to the browser client.
//! Used for local replay/spectate only — live matches are server-authoritative.

use wasm_bindgen::prelude::*;

use crucible_sim::serialize;
use crucible_sim::Replay;

/// Sim version string (also proves the sim crate linked into wasm).
#[wasm_bindgen]
pub fn sim_version() -> String {
    format!("crucible-sim {}", crucible_sim::VERSION)
}

/// Generate a map from a seed and return its symmetric HQ tiles as JSON.
#[wasm_bindgen]
pub fn map_hq_json(seed: u64) -> String {
    let map = crucible_sim::Map::generate(seed);
    serde_json::to_string(&map.hq_tiles).expect("infallible")
}

/// Re-run a replay to a given tick and return the game snapshot as JSON.
/// Deterministic: identical input produces byte-identical output on native
/// and wasm.
#[wasm_bindgen]
pub fn replay_snapshot_json(replay_json: &str, tick: i32) -> String {
    let replay: Replay = serde_json::from_str(replay_json).expect("valid replay");
    let mut game = serialize::replay_to_game(&replay);
    while game.tick < tick && !game.is_over() {
        game.step();
    }
    serialize::snapshot_json(&game)
}

/// Re-run a replay to completion and return the result plus a deterministic
/// snapshot hash (FNV-1a over the serialized final state). The hash lets the
/// browser verify native/wasm parity byte-for-byte.
#[wasm_bindgen]
pub fn replay_result(replay_json: &str) -> String {
    let replay: Replay = serde_json::from_str(replay_json).expect("valid replay");
    let game = serialize::finish_replay(&replay);
    let hash = fnv1a(&serialize::snapshot_bytes(&game));
    serde_json::json!({
        "winner": game.winner.map(|p| p.index() as u8),
        "reason": game.win_reason,
        "duration_ticks": game.tick,
        "hash": hash,
    })
    .to_string()
}

fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnv1a_native(data: &[u8]) -> u64 {
        fnv1a(data)
    }

    #[test]
    fn replay_result_matches_direct_run() {
        let seed = 7u64;
        let cfg = crucible_sim::GameConfig {
            timeout_ticks: 300,
            ..crucible_sim::GameConfig::default()
        };
        let replay = Replay::new(seed, cfg.clone());
        let result: serde_json::Value =
            serde_json::from_str(&replay_result(&replay.to_json())).unwrap();

        let mut game = crucible_sim::Game::new(crucible_sim::Map::generate(seed), cfg);
        while !game.is_over() {
            game.step();
        }
        assert_eq!(
            result["hash"].as_u64().unwrap(),
            fnv1a_native(&serialize::snapshot_bytes(&game))
        );
        assert_eq!(result["duration_ticks"].as_i64().unwrap() as i32, game.tick);
    }

    #[test]
    fn snapshot_at_tick_matches_direct_stepping() {
        let seed = 9u64;
        let cfg = crucible_sim::GameConfig {
            timeout_ticks: 100_000,
            ..crucible_sim::GameConfig::default()
        };
        let replay = Replay::new(seed, cfg.clone());
        let snap: serde_json::Value =
            serde_json::from_str(&replay_snapshot_json(&replay.to_json(), 250)).unwrap();

        let mut game = crucible_sim::Game::new(crucible_sim::Map::generate(seed), cfg);
        while game.tick < 250 {
            game.step();
        }
        let direct: serde_json::Value =
            serde_json::from_str(&serialize::snapshot_json(&game)).unwrap();
        assert_eq!(snap, direct);
    }
}
