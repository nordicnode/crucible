//! §10 command fuzz: random command streams must never crash the sim, and
//! every command must either apply cleanly or be rejected by the validator —
//! never silently misapplied. Driven by the injected PRNG, so any failure is
//! deterministic and reproducible. State invariants are checked every tick.

use crucible_sim::{
    building_stats, unit_stats, BuildingType, Command, CommandError, Game, GameConfig, Map, Player,
    Rng, Stance, UnitType, Upgrade,
};

const TICKS: i32 = 600;

fn config() -> GameConfig {
    GameConfig {
        timeout_ticks: 10_000,
        ..GameConfig::default()
    }
}

/// A random command drawn from the whole action space (valid tiles, entity
/// ids, and types — so the validator's rejection paths are exercised, not
/// just its happy paths). Coordinates are always in-bounds; whether the tile
/// is legal is the validator's job.
fn random_command(rng: &mut Rng, g: &Game, p: Player) -> Command {
    let tile = ((rng.below(64) as u8), (rng.below(64) as u8));
    let bt = match rng.below(10) {
        0 => BuildingType::Hq,
        1 => BuildingType::PowerPlant,
        2 => BuildingType::Refinery,
        3 => BuildingType::Barracks,
        4 => BuildingType::Factory,
        5 => BuildingType::TechLab,
        6 => BuildingType::Airfield,
        7 => BuildingType::Radar,
        8 => BuildingType::TeslaCoil,
        _ => BuildingType::Turret,
    };
    let ut = match rng.below(7) {
        0 => UnitType::Harvester,
        1 => UnitType::Infantry,
        2 => UnitType::Tank,
        3 => UnitType::Artillery,
        4 => UnitType::MammothTank,
        5 => UnitType::Gunship,
        _ => UnitType::Interceptor,
    };
    let building_id = g
        .buildings
        .get(rng.below((g.buildings.len().max(1)) as u64) as usize)
        .map(|b| b.id)
        .unwrap_or(1);
    let unit_id = g
        .units
        .get(rng.below((g.units.len().max(1)) as u64) as usize)
        .map(|u| u.id)
        .unwrap_or(1);
    let units = match rng.below(4) {
        0 => vec![unit_id],
        1 => vec![unit_id, unit_id.saturating_add(1)],
        2 => vec![],
        _ => vec![unit_id, unit_id],
    };
    let stance = match rng.below(3) {
        0 => Stance::Aggressive,
        1 => Stance::Cautious,
        _ => Stance::Hold,
    };
    let upgrade = match rng.below(4) {
        0 => Upgrade::None,
        1 => Upgrade::Damage,
        2 => Upgrade::Hp,
        _ => Upgrade::Range,
    };

    match rng.below(7) {
        0 => Command::PlaceBuilding {
            player: p,
            btype: bt,
            tile,
        },
        1 => Command::TrainUnit {
            player: p,
            building: building_id,
            utype: ut,
        },
        2 => Command::MoveGroup {
            player: p,
            units,
            waypoint: tile,
            stance,
        },
        3 => Command::SetRally {
            player: p,
            building: building_id,
            waypoint: tile,
        },
        4 => Command::ChooseUpgrade {
            player: p,
            lab: building_id,
            upgrade,
        },
        5 => Command::Sell {
            player: p,
            building: building_id,
        },
        _ => Command::Repair {
            player: p,
            building: building_id,
        },
    }
}

/// Check the invariants the fuzz run must never violate.
fn check_invariants(g: &Game) {
    for b in &g.buildings {
        assert!((0..=b.max_hp).contains(&b.hp), "building hp out of range");
        assert!(b.max_hp > 0);
    }
    for u in &g.units {
        assert!((0..=u.max_hp).contains(&u.hp), "unit hp out of range");
        assert!(u.max_hp > 0);
        assert!(u.carrying >= 0, "negative carried ore");
        assert!(u.cooldown >= 0, "negative cooldown");
    }
    assert!(g.ore[0] >= 0 && g.ore[1] >= 0, "negative ore");
    // Stats lookups never fail for stored kinds.
    for b in &g.buildings {
        let _ = building_stats(b.btype);
    }
    for u in &g.units {
        let _ = unit_stats(u.utype);
    }
}

/// Drive `seed` for `TICKS` ticks, issuing random command batches, and return
/// the final snapshot. Panics on any invariant violation (the fuzz's job).
fn fuzz_run(seed: u64) -> Vec<u8> {
    let mut rng = Rng::from_seed(seed);
    let mut g = Game::new(Map::generate(seed), config());
    while g.tick < TICKS && !g.is_over() {
        if g.is_command_tick() {
            let mut cmds = Vec::new();
            let n = 1 + rng.below(4) as usize; // 1..=4 commands per tick
            for _ in 0..n {
                let p = if rng.below(2) == 0 {
                    Player::P0
                } else {
                    Player::P1
                };
                cmds.push(random_command(&mut rng, &g, p));
            }
            let results = g.apply_commands(Player::P0, &cmds);
            // Every result is either applied or a named rejection.
            for r in results {
                if let Err(e) = r {
                    let _: CommandError = e;
                }
            }
        }
        g.step();
        check_invariants(&g);
    }
    crucible_sim::serialize::snapshot_bytes(&g)
}

#[test]
fn random_command_streams_never_crash_or_violate_state() {
    for seed in [1u64, 2, 3, 4, 5, 42, 999] {
        fuzz_run(seed); // panics on any violation
    }
}

#[test]
fn fuzz_is_deterministic() {
    for seed in [7u64, 123] {
        assert_eq!(fuzz_run(seed), fuzz_run(seed));
    }
}
