//! Deterministic scripted baseline bots: easy (turtle), medium (periodic
//! attack waves), hard (expand-and-push).
//!
//! These are **oracle baselines**: they may read the full [`Game`] (see
//! `CONTRACT.md` §5). They exist to bootstrap training, seed the gauntlet
//! baselines, and anchor the regression floor the learned champion must beat.
//! They are deterministic given a map seed and never exceed the sim's APM cap.

use crucible_sim::{
    building_stats, unit_stats, Building, BuildingType, Command, EntityId, Game, Player,
    Stance, UnitType, Upgrade,
};

use crate::bot::Bot;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn own_building(g: &Game, p: Player, bt: BuildingType) -> Option<&Building> {
    g.buildings.iter().find(|b| b.owner == p && b.btype == bt)
}

fn count_buildings(g: &Game, p: Player, bt: BuildingType) -> usize {
    g.buildings
        .iter()
        .filter(|b| b.owner == p && b.btype == bt)
        .count()
}

fn count_units(g: &Game, p: Player, ut: UnitType) -> usize {
    g.units
        .iter()
        .filter(|u| u.owner == p && u.utype == ut)
        .count()
}

fn can_afford(g: &Game, p: Player, cost: i32) -> bool {
    g.ore[p.index()] >= cost
}

pub(crate) fn combat_unit_ids(g: &Game, p: Player) -> Vec<EntityId> {
    g.units
        .iter()
        .filter(|u| u.owner == p && unit_stats(u.utype).damage > 0)
        .map(|u| u.id)
        .collect()
}

pub(crate) fn is_valid_build_tile(g: &Game, p: Player, bt: BuildingType, tile: (u8, u8)) -> bool {
    let cmd = Command::PlaceBuilding {
        player: p,
        btype: bt,
        tile,
    };
    g.validate_command(&cmd).is_ok()
}

/// Find a valid placement tile, searching outward from `preferred` in a
/// deterministic ring order.
pub(crate) fn find_build_tile(
    g: &Game,
    p: Player,
    bt: BuildingType,
    preferred: (u8, u8),
) -> Option<(u8, u8)> {
    if is_valid_build_tile(g, p, bt, preferred) {
        return Some(preferred);
    }
    for r in 1..=32i32 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue;
                }
                let x = preferred.0 as i32 + dx;
                let y = preferred.1 as i32 + dy;
                if !(0..64).contains(&x) || !(0..64).contains(&y) {
                    continue;
                }
                let t = (x as u8, y as u8);
                if is_valid_build_tile(g, p, bt, t) {
                    return Some(t);
                }
            }
        }
    }
    None
}

/// Place `bt` if the player has fewer than `max` of them and can afford it.
fn place_if_missing(
    g: &Game,
    p: Player,
    bt: BuildingType,
    preferred: (u8, u8),
    max: usize,
) -> Option<Command> {
    if count_buildings(g, p, bt) >= max {
        return None;
    }
    if !can_afford(g, p, building_stats(bt).cost) {
        return None;
    }
    let tile = find_build_tile(g, p, bt, preferred)?;
    Some(Command::PlaceBuilding {
        player: p,
        btype: bt,
        tile,
    })
}

/// Train `ut` from the producing building if under `target` and affordable.
///
/// `target` counts spawned units **plus** units still queued, so the bot does
/// not over-queue and blow its ore budget. When several producers exist (e.g.
/// hard's second factory) the least-loaded one is used, so production spreads
/// across factories instead of piling onto the first.
fn train_up_to(
    g: &Game,
    p: Player,
    producer: BuildingType,
    ut: UnitType,
    target: usize,
) -> Option<Command> {
    let queued: usize = g
        .buildings
        .iter()
        .filter(|b| b.owner == p && b.btype == producer)
        .map(|b| b.queue.iter().filter(|&&q| q == ut).count())
        .sum();
    if count_units(g, p, ut) + queued >= target {
        return None;
    }
    let building = g
        .buildings
        .iter()
        .filter(|b| b.owner == p && b.btype == producer && b.is_alive() && b.queue.len() < g.config.max_queue)
        .min_by_key(|b| b.queue.len())?;
    if !can_afford(g, p, unit_stats(ut).cost) {
        return None;
    }
    Some(Command::TrainUnit {
        player: p,
        building: building.id,
        utype: ut,
    })
}

/// Attack-move every combat unit toward a tile.
fn attack_move(g: &Game, p: Player, target: (u8, u8)) -> Option<Command> {
    let units = combat_unit_ids(g, p);
    if units.is_empty() {
        return None;
    }
    Some(Command::MoveGroup {
        player: p,
        units,
        waypoint: target,
        stance: Stance::Aggressive,
    })
}

/// Choose the damage upgrade once a Tech Lab exists.
fn choose_damage_upgrade(g: &Game, p: Player) -> Option<Command> {
    if g.upgrades[p.index()] != Upgrade::None {
        return None;
    }
    let lab = own_building(g, p, BuildingType::TechLab)?;
    Some(Command::ChooseUpgrade {
        player: p,
        lab: lab.id,
        upgrade: Upgrade::Damage,
    })
}

fn enemy_hq_tile(g: &Game, p: Player) -> (u8, u8) {
    if let Some(hq) = g.hq(p.enemy()) {
        return hq.tile;
    }
    g.map.hq_tiles[p.enemy().index()]
}

/// A tile `dist` tiles from `hq` toward `enemy` (clamped to the map).
fn toward_enemy(hq: (u8, u8), enemy: (u8, u8), dist: i32) -> (u8, u8) {
    let dx = (enemy.0 as i32 - hq.0 as i32).signum();
    let dy = (enemy.1 as i32 - hq.1 as i32).signum();
    (
        (hq.0 as i32 + dx * dist).clamp(0, 63) as u8,
        (hq.1 as i32 + dy * dist).clamp(0, 63) as u8,
    )
}

/// Symmetrically orient building offsets toward the quadrant's natural ore pocket.
fn base_offset(hq: (u8, u8), dx: i32, dy: i32) -> (u8, u8) {
    let sx = if hq.0 < 32 { dx } else { -dx };
    let sy = if hq.1 < 32 { dy } else { -dy };
    (
        (hq.0 as i32 + sx).clamp(0, 63) as u8,
        (hq.1 as i32 + sy).clamp(0, 63) as u8,
    )
}

// ---------------------------------------------------------------------------
// Easy — passive turtle
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct EasyBot {
    /// Number of turrets placed so far; the turtle never rebuilds them, so
    /// sustained waves eventually break through.
    built_turrets: u8,
}

impl Bot for EasyBot {
    fn name(&self) -> &'static str {
        "easy"
    }

    fn decide(&mut self, g: &Game, p: Player) -> Vec<Command> {
        let mut out = Vec::new();
        let hq = g.hq(p).map(|b| b.tile).unwrap_or((8, 8));

        // Economy first: refinery + factory + the opening harvesters. The
        // factory must precede the barracks or the turtle never gets income.
        if let Some(c) = place_if_missing(g, p, BuildingType::Refinery, base_offset(hq, 2, 0), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::Factory, base_offset(hq, 0, 2), 1) {
            out.push(c);
        }
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Harvester, 4) {
            out.push(c);
        }

        // Defense once income is running: a few infantry + one enemy-facing
        // turret slow the opening rush; the turtle never attacks, so these
        // delay the inevitable rather than win.
        if own_building(g, p, BuildingType::Factory).is_some() {
            if let Some(c) = place_if_missing(g, p, BuildingType::Barracks, base_offset(hq, 2, 2), 1)
            {
                out.push(c);
            }
            if let Some(c) = train_up_to(g, p, BuildingType::Barracks, UnitType::Infantry, 7) {
                out.push(c);
            }
            if g.tick > 400 && self.built_turrets < 1 {
                let t = toward_enemy(hq, enemy_hq_tile(g, p), 2);
                if let Some(c) = place_if_missing(g, p, BuildingType::Turret, t, 1) {
                    out.push(c);
                }
            }
            if g.tick > 1_400 && self.built_turrets < 2 {
                let t = toward_enemy(hq, enemy_hq_tile(g, p), 3);
                if let Some(c) = place_if_missing(g, p, BuildingType::Turret, t, 2) {
                    out.push(c);
                }
            }
            self.built_turrets = self
                .built_turrets
                .max(count_buildings(g, p, BuildingType::Turret) as u8);
        }

        // Scale the economy.
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Harvester, 8) {
            out.push(c);
        }
        out
    }
}

/// Public constructor.
pub fn easy() -> EasyBot {
    EasyBot::default()
}

// ---------------------------------------------------------------------------
// Medium — periodic attack waves
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MediumBot {
    last_attack: i32,
}

impl Bot for MediumBot {
    fn name(&self) -> &'static str {
        "medium"
    }

    fn decide(&mut self, g: &Game, p: Player) -> Vec<Command> {
        let mut out = Vec::new();
        let hq = g.hq(p).map(|b| b.tile).unwrap_or((8, 8));

        if let Some(c) = place_if_missing(g, p, BuildingType::Refinery, base_offset(hq, 2, 0), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::Factory, base_offset(hq, 0, 2), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::PowerPlant, base_offset(hq, 0, -2), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::Barracks, base_offset(hq, 2, 2), 1) {
            out.push(c);
        }
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Harvester, 6) {
            out.push(c);
        }
        if let Some(c) = train_up_to(g, p, BuildingType::Barracks, UnitType::Infantry, 8) {
            out.push(c);
        }
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Tank, 4) {
            out.push(c);
        }

        // Rush early with a small force, then keep sending waves.
        let combat = combat_unit_ids(g, p).len();
        let ready = combat >= 3;
        let interval_elapsed = g.tick - self.last_attack >= 600;
        if ready && (self.last_attack == 0 || interval_elapsed) {
            if let Some(c) = attack_move(g, p, enemy_hq_tile(g, p)) {
                out.push(c);
                self.last_attack = g.tick;
            }
        }
        out
    }
}

/// Public constructor.
pub fn medium() -> MediumBot {
    MediumBot::default()
}

// ---------------------------------------------------------------------------
// Hard — expand-and-push
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct HardBot {
    last_attack: i32,
}

impl Bot for HardBot {
    fn name(&self) -> &'static str {
        "hard"
    }

    fn decide(&mut self, g: &Game, p: Player) -> Vec<Command> {
        let mut out = Vec::new();
        let hq = g.hq(p).map(|b| b.tile).unwrap_or((8, 8));

        // Core production buildings.
        if let Some(c) = place_if_missing(g, p, BuildingType::Refinery, base_offset(hq, 2, 0), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::Factory, base_offset(hq, 0, 2), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::PowerPlant, base_offset(hq, 0, -2), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::Barracks, base_offset(hq, 2, 2), 1) {
            out.push(c);
        }

        // 2. Economy: 6 harvesters.
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Harvester, 6) {
            out.push(c);
        }

        // 3. Early army: 8 infantry + 6 tanks.
        if let Some(c) = train_up_to(g, p, BuildingType::Barracks, UnitType::Infantry, 8) {
            out.push(c);
        }
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Tank, 6) {
            out.push(c);
        }

        // 4. Tech Lab & Damage Upgrade (+15% attack power for all units).
        if let Some(c) = place_if_missing(g, p, BuildingType::TechLab, base_offset(hq, -2, 2), 1) {
            out.push(c);
        }
        if let Some(c) = choose_damage_upgrade(g, p) {
            out.push(c);
        }

        // 5. Dual Factory mass production.
        if own_building(g, p, BuildingType::TechLab).is_some() {
            if let Some(c) = place_if_missing(g, p, BuildingType::Factory, base_offset(hq, 0, 4), 2) {
                out.push(c);
            }
        }

        // 6. Mass late-game armor: 14 tanks + 4 artillery.
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Tank, 14) {
            out.push(c);
        }
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Artillery, 4) {
            out.push(c);
        }

        // 7. Tactical push: attack with crushing numbers.
        let combat = combat_unit_ids(g, p).len();
        let ready = combat >= 4;
        let interval_elapsed = g.tick - self.last_attack >= 400;
        if ready && (self.last_attack == 0 || interval_elapsed) {
            if let Some(c) = attack_move(g, p, enemy_hq_tile(g, p)) {
                out.push(c);
                self.last_attack = g.tick;
            }
        }
        out
    }
}

/// Public constructor.
pub fn hard() -> HardBot {
    HardBot::default()
}
