//! Replay format test: record an input log, serialize it to JSON, reload it,
//! and reproduce the match byte-identically. This is the property every
//! stored match depends on.

use crucible_sim::{
    Command, EntityId, Game, GameConfig, Map, Player, Replay, ReplayResult, Stance, UnitType,
};

/// A small deterministic script: both players build a base, produce units,
/// then skirmish toward the enemy HQ.
fn scripted_replay(seed: u64) -> Replay {
    let cfg = GameConfig {
        starting_ore: 2_000,
        timeout_ticks: 100_000,
        ..GameConfig::default()
    };
    let mut g = Game::new(Map::generate(seed), cfg.clone());
    let mut replay = Replay::new(seed, cfg);

    while !g.is_over() && g.tick < 3_000 {
        if g.is_command_tick() {
            for p in Player::ALL {
                let cmds = script_commands(&g, p);
                for c in &cmds {
                    replay.record(g.tick, p, c.clone());
                }
                g.apply_commands(p, &cmds);
            }
        }
        g.step();
    }

    replay.result = Some(ReplayResult {
        winner: g.winner,
        reason: g.win_reason,
        duration_ticks: g.tick,
    });
    replay
}

fn script_commands(g: &Game, p: Player) -> Vec<Command> {
    let mut cmds = Vec::new();
    let hq = g.hq(p).map(|b| b.tile).unwrap_or((8, 8));

    if g.tick == 0 {
        for (bt, tile) in [
            (crucible_sim::BuildingType::Refinery, (hq.0 + 2, hq.1)),
            (crucible_sim::BuildingType::Factory, (hq.0, hq.1 + 2)),
            (crucible_sim::BuildingType::Barracks, (hq.0 + 2, hq.1 + 2)),
        ] {
            cmds.push(Command::PlaceBuilding {
                player: p,
                btype: bt,
                tile,
            });
        }
    }

    let harvesters = g
        .units
        .iter()
        .filter(|u| u.owner == p && u.utype == UnitType::Harvester)
        .count();
    if harvesters < 2 {
        if let Some(b) = g
            .buildings
            .iter()
            .find(|b| b.owner == p && b.btype == crucible_sim::BuildingType::Factory)
        {
            if g.ore[p.index()] >= 100 {
                cmds.push(Command::TrainUnit {
                    player: p,
                    building: b.id,
                    utype: UnitType::Harvester,
                });
            }
        }
    }
    let infantry = g
        .units
        .iter()
        .filter(|u| u.owner == p && u.utype == UnitType::Infantry)
        .count();
    if infantry < 3 {
        if let Some(b) = g
            .buildings
            .iter()
            .find(|b| b.owner == p && b.btype == crucible_sim::BuildingType::Barracks)
        {
            if g.ore[p.index()] >= 50 {
                cmds.push(Command::TrainUnit {
                    player: p,
                    building: b.id,
                    utype: UnitType::Infantry,
                });
            }
        }
    }

    // Skirmish once some army exists.
    if g.tick == 2_000 {
        let combat: Vec<EntityId> = g
            .units
            .iter()
            .filter(|u| u.owner == p && crucible_sim::unit_stats(u.utype).damage > 0)
            .map(|u| u.id)
            .collect();
        if !combat.is_empty() {
            let enemy = g.hq(p.enemy()).map(|b| b.tile).unwrap_or((55, 55));
            cmds.push(Command::MoveGroup {
                player: p,
                units: combat,
                waypoint: enemy,
                stance: Stance::Aggressive,
            });
        }
    }

    cmds
}

#[test]
fn replay_reproduces_state_byte_identical() {
    for seed in [7u64, 123, 999] {
        let original = scripted_replay(seed);
        let json = original.to_json();
        let loaded: Replay = Replay::from_json(&json).expect("replay JSON round-trips");

        let mut repro = crucible_sim::serialize::replay_to_game(&loaded);
        let target = original
            .result
            .as_ref()
            .map(|r| r.duration_ticks)
            .unwrap_or(0);
        while repro.tick < target && !repro.is_over() {
            repro.step();
        }

        let a = crucible_sim::serialize::snapshot_bytes(&repro);

        // Independently re-run the same commands straight from the struct.
        let mut reference = crucible_sim::serialize::replay_to_game(&original);
        while reference.tick < target && !reference.is_over() {
            reference.step();
        }
        let b = crucible_sim::serialize::snapshot_bytes(&reference);

        assert_eq!(a, b, "JSON round-trip diverged for seed {seed}");
        assert_eq!(loaded.commands.len(), original.commands.len());
    }
}

#[test]
fn replay_is_small_input_log() {
    // A replay must not be a state dump: the JSON must stay in the KB range.
    let replay = scripted_replay(42);
    let json = replay.to_json();
    assert!(
        json.len() < 50_000,
        "replay unexpectedly large: {} bytes",
        json.len()
    );
}
