//! wasm-bindgen shim exposing the deterministic sim to the browser client.
//! Used for local replay/spectate only — live matches are server-authoritative.

use wasm_bindgen::prelude::*;

use crucible_sim::fixed::FIX_SCALE;
use crucible_sim::serialize;
use crucible_sim::{Map, Replay};

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
/// and wasm. Supports seeking to any tick (forward or backward).
#[wasm_bindgen]
pub fn replay_snapshot_json(replay_json: &str, tick: i32) -> String {
    let replay: Replay = serde_json::from_str(replay_json).expect("valid replay");
    let game = serialize::replay_at_tick(&replay, tick);
    serialize::snapshot_json(&game)
}

/// Static replay metadata for the spectate screen: the map (passability, HQ
/// spawns, initial ore layout) plus the recorded outcome. Called once per
/// replay; the per-frame payload in [`replay_frame`] stays lean.
#[wasm_bindgen]
pub fn replay_meta(replay_json: &str) -> String {
    let replay: Replay = serde_json::from_str(replay_json).expect("valid replay");
    let map = Map::generate(replay.map_seed);
    let duration = replay
        .result
        .as_ref()
        .map(|r| r.duration_ticks)
        .unwrap_or(replay.config.timeout_ticks);
    serde_json::json!({
        "map_seed": replay.map_seed,
        "passable": map.passable,
        "hq_tiles": map.hq_tiles,
        "ore": map.ore,
        "duration_ticks": duration,
        "winner": replay.result.as_ref().and_then(|r| r.winner.map(|p| p.index() as u8)),
        "win_reason": replay.result.as_ref().and_then(|r| r.reason),
    })
    .to_string()
}

/// One lean spectate frame: both players' entities (full state, no fog) and
/// scores at a given tick. `kind` strings use the serde variant names
/// (`"Infantry"`, `"Hq"`, …) to match the live match protocol.
#[wasm_bindgen]
pub fn replay_frame(replay_json: &str, tick: i32) -> String {
    let replay: Replay = serde_json::from_str(replay_json).expect("valid replay");
    let game = serialize::replay_at_tick(&replay, tick);
    let units: Vec<serde_json::Value> = game
        .units
        .iter()
        .map(|u| {
            serde_json::json!({
                "id": u.id,
                "kind": u.utype,
                "owner": u.owner,
                "x": u.pos.x as f32 / FIX_SCALE as f32,
                "y": u.pos.y as f32 / FIX_SCALE as f32,
                "hp": u.hp,
                "max_hp": u.max_hp,
            })
        })
        .collect();
    let buildings: Vec<serde_json::Value> = game
        .buildings
        .iter()
        .map(|b| {
            serde_json::json!({
                "id": b.id,
                "kind": b.btype,
                "owner": b.owner,
                "x": b.tile.0 as f32 + 0.5,
                "y": b.tile.1 as f32 + 0.5,
                "hp": b.hp,
                "max_hp": b.max_hp,
            })
        })
        .collect();
    serde_json::json!({
        "tick": game.tick,
        "ore0": game.ore[0],
        "ore1": game.ore[1],
        "units": units,
        "buildings": buildings,
        "winner": game.winner.map(|p| p.index() as u8),
        "win_reason": game.win_reason,
    })
    .to_string()
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

    #[test]
    fn replay_frame_and_meta_match_snapshot() {
        use crucible_sim::{BuildingType, Command, GameConfig, Player};

        let seed = 21u64;
        let cfg = GameConfig {
            timeout_ticks: 500,
            ..GameConfig::default()
        };
        let mut replay = Replay::new(seed, cfg.clone());
        let hq = crucible_sim::Map::generate(seed).hq_tiles[0];
        replay.record(
            0,
            Player::P0,
            Command::PlaceBuilding {
                player: Player::P0,
                btype: BuildingType::Refinery,
                tile: (hq.0 + 2, hq.1),
            },
        );
        let rj = replay.to_json();

        let meta: serde_json::Value = serde_json::from_str(&replay_meta(&rj)).unwrap();
        assert_eq!(meta["map_seed"].as_u64().unwrap(), seed);
        assert_eq!(meta["passable"].as_array().unwrap().len(), 64 * 64);

        let snap: serde_json::Value = serde_json::from_str(&replay_snapshot_json(&rj, 50)).unwrap();
        let frame: serde_json::Value = serde_json::from_str(&replay_frame(&rj, 50)).unwrap();
        assert_eq!(frame["tick"].as_i64().unwrap(), 50);
        assert_eq!(
            snap["units"].as_array().unwrap().len(),
            frame["units"].as_array().unwrap().len()
        );
        assert_eq!(
            snap["buildings"].as_array().unwrap().len(),
            frame["buildings"].as_array().unwrap().len()
        );
        // Kind strings are serde variant names (capitalized) — what the client
        // renderer expects.
        let kinds: Vec<&str> = frame["buildings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["kind"].as_str().unwrap())
            .collect();
        assert!(kinds.contains(&"Hq") && kinds.contains(&"Refinery"));
    }
}
