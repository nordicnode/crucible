//! Early M2 sanity scenarios: prove the economy loop runs and that the unit
//! counter relationships the design promises actually hold in the sim.

use crucible_sim::{
    building_stats, open_test_map, unit_stats, BuildingType, Command, EventKind, Game, GameConfig,
    Player, Pos, Stance, Unit, UnitOrder, UnitType,
};

fn open_game(starting_ore: i32) -> Game {
    let cfg = GameConfig {
        starting_ore,
        timeout_ticks: 100_000,
        ..GameConfig::default()
    };
    Game::new(open_test_map(7), cfg)
}

fn spawn_unit(g: &mut Game, p: Player, ut: UnitType, tile: (u8, u8)) -> u32 {
    let stats = unit_stats(ut);
    let id = g.alloc_id();
    g.units.push(Unit {
        id,
        owner: p,
        utype: ut,
        pos: Pos::from_tile(tile.0, tile.1),
        hp: stats.hp,
        max_hp: stats.hp,
        stance: Stance::Aggressive,
        order: UnitOrder::Idle,
        carrying: 0,
        cooldown: 0,
        mining: 0,
        path: vec![],
        target: None,
        fleeing: false,
        harvest_tile: None,
        refinery: None,
    });
    id
}

fn spawn_building(g: &mut Game, p: Player, bt: BuildingType, tile: (u8, u8)) -> u32 {
    let stats = building_stats(bt);
    let id = g.alloc_id();
    g.buildings.push(crucible_sim::Building {
        id,
        owner: p,
        btype: bt,
        tile,
        hp: stats.hp,
        max_hp: stats.hp,
        queue: vec![],
        progress: 0,
        rally: None,
        cooldown: 0,
    });
    id
}

#[test]
fn harvester_mines_and_deposits() {
    let mut g = open_game(1000);
    let hq = g.hq(Player::P0).unwrap().tile;
    let _ = g.apply_commands(
        Player::P0,
        &[
            Command::PlaceBuilding {
                player: Player::P0,
                btype: BuildingType::Refinery,
                tile: (hq.0 + 2, hq.1),
            },
            Command::PlaceBuilding {
                player: Player::P0,
                btype: BuildingType::Factory,
                tile: (hq.0, hq.1 + 2),
            },
        ],
    );
    let factory = g
        .buildings
        .iter()
        .find(|b| b.owner == Player::P0 && b.btype == BuildingType::Factory)
        .unwrap()
        .id;
    let _ = g.apply_commands(
        Player::P0,
        &[Command::TrainUnit {
            player: Player::P0,
            building: factory,
            utype: UnitType::Harvester,
        }],
    );

    let ore_before: i32 = g.map.ore.iter().sum();
    for _ in 0..900 {
        g.step();
    }
    let ore_after: i32 = g.map.ore.iter().sum();

    assert!(
        ore_after < ore_before,
        "harvester never mined: {ore_before} -> {ore_after}"
    );
    let deposited = g.events.iter().any(|e| {
        matches!(
            e.kind,
            EventKind::OreDeposited {
                player: Player::P0,
                ..
            }
        )
    });
    assert!(deposited, "harvester never returned ore to the refinery");
}

#[test]
fn tanks_beat_equal_cost_infantry() {
    let mut g = open_game(0);
    // 3 tanks (450 ore) vs 9 infantry (450 ore), clustered so splash matters.
    for i in 0..3 {
        spawn_unit(&mut g, Player::P0, UnitType::Tank, (30, 29 + i as u8));
    }
    for i in 0..9 {
        spawn_unit(&mut g, Player::P1, UnitType::Infantry, (34, 28 + i as u8));
    }
    for _ in 0..1200 {
        g.step();
    }
    let p1_infantry = g
        .units
        .iter()
        .filter(|u| u.owner == Player::P1 && u.utype == UnitType::Infantry)
        .count();
    let p0_tanks = g
        .units
        .iter()
        .filter(|u| u.owner == Player::P0 && u.utype == UnitType::Tank)
        .count();
    assert_eq!(p1_infantry, 0, "infantry survived the tank push");
    assert!(p0_tanks > 0, "tanks were wiped out");
}

#[test]
fn artillery_outranges_turret() {
    let mut g = open_game(0);
    spawn_unit(&mut g, Player::P0, UnitType::Artillery, (30, 30));
    spawn_building(&mut g, Player::P1, BuildingType::Turret, (34, 30));

    for _ in 0..800 {
        g.step();
    }

    let turret_alive = g
        .buildings
        .iter()
        .any(|b| b.btype == BuildingType::Turret && b.owner == Player::P1);
    let artillery_alive = g
        .units
        .iter()
        .any(|u| u.utype == UnitType::Artillery && u.owner == Player::P0);
    assert!(!turret_alive, "turret survived artillery siege");
    assert!(artillery_alive, "artillery died to a turret it outranges");
}
