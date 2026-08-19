//! #[doc(hidden)] — the shared determinism-golden scenario.
//!
//! The golden hashes pin the *exact* serialized state of a scripted match at
//! specific ticks. Both the native test (`tests/determinism.rs`) and the wasm
//! test (`crucible-client-wasm/tests/wasm_parity.rs`) call [`golden_hashes`]
//! and compare against the same constants, so native/wasm parity is proven on
//! identical code paths rather than two hand-kept copies.

use crate::entity::{unit_stats, BuildingType, EntityId, Player, Stance, UnitType};
use crate::serialize::snapshot_bytes;
use crate::{Command, Game, GameConfig, Map};

/// The seed the golden scenario runs on.
pub const SEED: u64 = 12345;

/// Golden snapshot hashes (FNV-1a over `serialize::snapshot_bytes`).
/// Recorded after auditing the sim for the v1 determinism contract; if any
/// change alters sim behavior these change and the tests fail.
pub const GOLDEN_100: u64 = 16562178345104678055;
pub const GOLDEN_1000: u64 = 16475545533167463206;
pub const GOLDEN_10000: u64 = 7841076763393545594;

pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

pub fn hash_snapshot(g: &Game) -> u64 {
    fnv1a(&snapshot_bytes(g))
}

fn find_building(g: &Game, p: Player, bt: BuildingType) -> EntityId {
    g.buildings
        .iter()
        .find(|b| b.owner == p && b.btype == bt)
        .map(|b| b.id)
        .expect("building missing")
}

/// Construct the scripted opening (bases + mixed army queues) at tick 0.
pub fn build_game(seed: u64) -> Game {
    let cfg = GameConfig {
        starting_ore: 100_000,
        timeout_ticks: 100_000,
        ..GameConfig::default()
    };
    let mut g = Game::new(Map::generate(seed), cfg);

    for p in Player::ALL {
        let (hx, hy) = g.hq(p).unwrap().tile;
        let placements = [
            (BuildingType::PowerPlant, (hx as i32 - 2, hy as i32 - 2)),
            (BuildingType::Refinery, (hx as i32 + 2, hy as i32)),
            (BuildingType::Factory, (hx as i32, hy as i32 + 2)),
            (BuildingType::Barracks, (hx as i32 + 2, hy as i32 + 2)),
            (BuildingType::Turret, (hx as i32 - 2, hy as i32)),
            (BuildingType::TechLab, (hx as i32, hy as i32 - 2)),
        ];
        for (bt, (x, y)) in placements {
            let tile = (x.clamp(0, 63) as u8, y.clamp(0, 63) as u8);
            let _ = g.apply_commands(
                p,
                &[Command::PlaceBuilding {
                    player: p,
                    btype: bt,
                    tile,
                }],
            );
        }
    }

    for p in Player::ALL {
        let factory = find_building(&g, p, BuildingType::Factory);
        let barracks = find_building(&g, p, BuildingType::Barracks);
        let cmds = [
            Command::TrainUnit {
                player: p,
                building: factory,
                utype: UnitType::Harvester,
            },
            Command::TrainUnit {
                player: p,
                building: factory,
                utype: UnitType::Harvester,
            },
            Command::TrainUnit {
                player: p,
                building: factory,
                utype: UnitType::Tank,
            },
            Command::TrainUnit {
                player: p,
                building: factory,
                utype: UnitType::Tank,
            },
            Command::TrainUnit {
                player: p,
                building: factory,
                utype: UnitType::Artillery,
            },
            Command::TrainUnit {
                player: p,
                building: barracks,
                utype: UnitType::Infantry,
            },
            Command::TrainUnit {
                player: p,
                building: barracks,
                utype: UnitType::Infantry,
            },
            Command::TrainUnit {
                player: p,
                building: barracks,
                utype: UnitType::Infantry,
            },
        ];
        let _ = g.apply_commands(p, &cmds);
    }

    g
}

/// Walk the armies to a neutral spot (exercises pathfinding without triggering
/// combat), issued at tick 1200.
pub fn issue_move_orders(g: &mut Game) {
    for p in Player::ALL {
        let hq = g.hq(p).unwrap().tile;
        let combat: Vec<EntityId> = g
            .units
            .iter()
            .filter(|u| u.owner == p && unit_stats(u.utype).damage > 0)
            .map(|u| u.id)
            .collect();
        if !combat.is_empty() {
            let wp = (
                (hq.0 as i32 + 8).clamp(0, 63) as u8,
                (hq.1 as i32 + 8).clamp(0, 63) as u8,
            );
            let _ = g.apply_commands(
                p,
                &[Command::MoveGroup {
                    player: p,
                    units: combat,
                    waypoint: wp,
                    stance: Stance::Aggressive,
                }],
            );
        }
    }
}

pub fn playout(seed: u64, target_tick: i32) -> Game {
    let mut g = build_game(seed);
    while g.tick < target_tick && !g.is_over() {
        if g.tick == 1200 {
            issue_move_orders(&mut g);
        }
        g.step();
    }
    g
}

/// Send each side's combat units toward the *enemy* HQ so the two armies meet
/// and fight — exercising combat resolution (targeting, splash, focus-fire),
/// which the movement-only scenario above deliberately avoids.
pub fn issue_attack_orders(g: &mut Game) {
    for p in Player::ALL {
        let enemy_hq = g.hq(p.enemy()).unwrap().tile;
        let combat: Vec<EntityId> = g
            .units
            .iter()
            .filter(|u| u.owner == p && unit_stats(u.utype).damage > 0)
            .map(|u| u.id)
            .collect();
        if !combat.is_empty() {
            let _ = g.apply_commands(
                p,
                &[Command::MoveGroup {
                    player: p,
                    units: combat,
                    waypoint: enemy_hq,
                    stance: Stance::Aggressive,
                }],
            );
        }
    }
}

/// A second golden scenario whose playout includes a real battle (both armies
/// attack each other), pinning the combat subsystem's determinism.
pub fn combat_playout(seed: u64, target_tick: i32) -> Game {
    let mut g = build_game(seed);
    while g.tick < target_tick && !g.is_over() {
        if g.tick == 1200 {
            issue_attack_orders(&mut g);
        }
        g.step();
    }
    g
}

/// Combat-scenario golden hashes in `[tick 1500, tick 1900, tick 2400]` order.
/// These ticks are deliberately mid-battle (units are dying across them), so
/// the hashes pin combat resolution — targeting, focus-fire, and tank splash —
/// rather than the pre/post-battle approach states.
pub fn combat_hashes() -> [u64; 3] {
    [
        hash_snapshot(&combat_playout(SEED, 1_500)),
        hash_snapshot(&combat_playout(SEED, 1_900)),
        hash_snapshot(&combat_playout(SEED, 2_400)),
    ]
}

/// The committed combat-golden values, in the same order as [`combat_hashes`].
pub const COMBAT_GOLDEN: [u64; 3] = [
    18192032058396949243,
    7744180762066791415,
    8315480985280236347,
];

/// The three golden snapshot hashes in `[tick 100, tick 1000, tick 10000]`
/// order. Deterministic on native and wasm.
pub fn golden_hashes() -> [u64; 3] {
    [
        hash_snapshot(&playout(SEED, 100)),
        hash_snapshot(&playout(SEED, 1_000)),
        hash_snapshot(&playout(SEED, 10_000)),
    ]
}

/// The committed golden values, in the same order as [`golden_hashes`].
pub const GOLDEN: [u64; 3] = [GOLDEN_100, GOLDEN_1000, GOLDEN_10000];
