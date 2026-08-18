//! M1 acceptance tests: cross-run byte-identical snapshots (golden hashes) and
//! map fairness invariants.
//!
//! The golden hashes pin the *exact* serialized state of a scripted match at
//! specific ticks. If any change alters sim behavior (float use, HashMap
//! iteration, entity order, rand drift, ...) these hashes change and fail.

use crucible_sim::{
    building_stats, unit_stats, BuildingType, Command, EntityId, Game, GameConfig, Map, Player,
    Stance, UnitType,
};

fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn hash_snapshot(g: &Game) -> u64 {
    fnv1a(&crucible_sim::serialize::snapshot_bytes(g))
}

fn find_building(g: &Game, p: Player, bt: BuildingType) -> EntityId {
    g.buildings
        .iter()
        .find(|b| b.owner == p && b.btype == bt)
        .map(|b| b.id)
        .expect("building missing")
}

/// Construct the scripted opening (bases + mixed army queues) at tick 0.
fn build_game(seed: u64) -> Game {
    let cfg = GameConfig {
        starting_ore: 100_000,
        timeout_ticks: 100_000,
        ..GameConfig::default()
    };
    let mut g = Game::new(Map::generate(seed), cfg);

    for p in Player::ALL {
        let (hx, hy) = g.hq(p).unwrap().tile;
        let placements = [
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

/// Walk the armies to opposite neutral corners (exercises pathfinding without
/// triggering combat), issued at tick 1200.
fn issue_move_orders(g: &mut Game) {
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

fn playout(seed: u64, target_tick: i32) -> Game {
    let mut g = build_game(seed);
    while g.tick < target_tick && !g.is_over() {
        if g.tick == 1200 {
            issue_move_orders(&mut g);
        }
        g.step();
    }
    g
}

#[test]
fn golden_snapshots_are_stable() {
    // Recorded after auditing the sim for the v1 determinism contract.
    const GOLDEN_100: u64 = 1961289812933995130;
    const GOLDEN_1000: u64 = 187858308740134226;
    const GOLDEN_10000: u64 = 9035103704727911103;

    let got = [
        hash_snapshot(&playout(12345, 100)),
        hash_snapshot(&playout(12345, 1_000)),
        hash_snapshot(&playout(12345, 10_000)),
    ];
    println!("golden hashes: {got:?}");

    assert_eq!(got[0], GOLDEN_100, "tick 100 hash changed");
    assert_eq!(got[1], GOLDEN_1000, "tick 1000 hash changed");
    assert_eq!(got[2], GOLDEN_10000, "tick 10000 hash changed");
}

#[test]
fn same_seed_replays_byte_identical() {
    for tick in [1i32, 50, 200, 999, 5_000] {
        let ga = playout(777, tick);
        let gb = playout(777, tick);
        assert_eq!(
            crucible_sim::serialize::snapshot_bytes(&ga),
            crucible_sim::serialize::snapshot_bytes(&gb),
            "divergence at tick {tick}"
        );
    }
}

#[test]
fn map_fairness_over_10k_seeds() {
    for seed in 0..10_000u64 {
        let map = Map::generate(seed);
        for idx in 0..(64 * 64) {
            let (x, y) = (idx % 64, idx / 64);
            let midx = (63 - y) * 64 + (63 - x);
            assert_eq!(
                map.passable[idx], map.passable[midx],
                "passable asymmetry seed {seed}"
            );
            assert_eq!(map.ore[idx], map.ore[midx], "ore asymmetry seed {seed}");
        }
        assert_eq!(
            map.hq_tiles[0],
            (63 - map.hq_tiles[1].0, 63 - map.hq_tiles[1].1),
            "HQ mirror seed {seed}"
        );
        assert!(map.is_passable(map.hq_tiles[0].0, map.hq_tiles[0].1));
        assert!(map.is_passable(map.hq_tiles[1].0, map.hq_tiles[1].1));
    }
}

#[allow(dead_code)]
fn _balance_refs() {
    let _ = building_stats(BuildingType::Hq).hp;
    let _ = unit_stats(UnitType::Tank).cost;
}
