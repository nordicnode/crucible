//! Deterministic scripted baseline bots: easy (turtle), medium (periodic
//! attack waves), hard (expand-and-push).
//!
//! These are **oracle baselines**: they may read the full [`Game`] (see
//! `CONTRACT.md` §5). They exist to bootstrap training, seed the gauntlet
//! baselines, and anchor the regression floor the learned champion must beat.
//! They are deterministic given a map seed and never exceed the sim's APM cap.

use crucible_sim::{
    building_stats, unit_stats, Building, BuildingType, Command, EntityId, Game, Player, Pos,
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
/// not over-queue and blow its ore budget.
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
    let building = own_building(g, p, producer)?;
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
    g.hq(p.enemy()).map(|b| b.tile).unwrap_or((55, 55))
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
        if let Some(c) = place_if_missing(g, p, BuildingType::Refinery, (hq.0 + 2, hq.1), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::Factory, (hq.0, hq.1 + 2), 1) {
            out.push(c);
        }
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Harvester, 4) {
            out.push(c);
        }

        // Defense once income is running: a few infantry + one enemy-facing
        // turret slow the opening rush; the turtle never attacks, so these
        // delay the inevitable rather than win.
        if own_building(g, p, BuildingType::Factory).is_some() {
            if let Some(c) = place_if_missing(g, p, BuildingType::Barracks, (hq.0 + 2, hq.1 + 2), 1)
            {
                out.push(c);
            }
            if let Some(c) = train_up_to(g, p, BuildingType::Barracks, UnitType::Infantry, 6) {
                out.push(c);
            }
            if g.tick > 800 && self.built_turrets < 1 {
                let t = toward_enemy(hq, enemy_hq_tile(g, p), 2);
                if let Some(c) = place_if_missing(g, p, BuildingType::Turret, t, 1) {
                    out.push(c);
                }
            }
            if g.tick > 1_800 && self.built_turrets < 2 {
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

        if let Some(c) = place_if_missing(g, p, BuildingType::Refinery, (hq.0 + 2, hq.1), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::Factory, (hq.0, hq.1 + 2), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::Barracks, (hq.0 + 2, hq.1 + 2), 1) {
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
        if let Some(c) = place_if_missing(g, p, BuildingType::Refinery, (hq.0 + 2, hq.1), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::Factory, (hq.0, hq.1 + 2), 1) {
            out.push(c);
        }
        if let Some(c) = place_if_missing(g, p, BuildingType::Barracks, (hq.0 + 2, hq.1 + 2), 1) {
            out.push(c);
        }

        // Early defense: cheap infantry plus turrets ringing the base.
        if let Some(c) = train_up_to(g, p, BuildingType::Barracks, UnitType::Infantry, 4) {
            out.push(c);
        }
        let turret_spots = [
            (hq.0.wrapping_sub(2), hq.1),
            (hq.0.wrapping_add(2), hq.1),
            (hq.0, hq.1.wrapping_sub(2)),
            (hq.0, hq.1.wrapping_add(2)),
        ];
        let turrets = count_buildings(g, p, BuildingType::Turret);
        if turrets < 2 {
            if let Some(tile) = find_build_tile(g, p, BuildingType::Turret, turret_spots[turrets]) {
                out.push(Command::PlaceBuilding {
                    player: p,
                    btype: BuildingType::Turret,
                    tile,
                });
            }
        }

        // Economy behind the defense.
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Harvester, 8) {
            out.push(c);
        }

        // Tech.
        if let Some(c) = place_if_missing(g, p, BuildingType::TechLab, (hq.0, hq.1 - 2), 1) {
            out.push(c);
        }
        if let Some(c) = choose_damage_upgrade(g, p) {
            out.push(c);
        }

        // Expand with a second refinery.
        if count_buildings(g, p, BuildingType::Refinery) < 2 {
            let pref = expansion_preferred(g, p).unwrap_or((hq.0 + 4, hq.1));
            let tile = find_build_tile(g, p, BuildingType::Refinery, pref)
                .or_else(|| find_build_tile(g, p, BuildingType::Refinery, (hq.0 + 4, hq.1)));
            if let Some(tile) = tile {
                out.push(Command::PlaceBuilding {
                    player: p,
                    btype: BuildingType::Refinery,
                    tile,
                });
            }
        }

        // Heavy army.
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Tank, 9) {
            out.push(c);
        }
        if let Some(c) = train_up_to(g, p, BuildingType::Factory, UnitType::Artillery, 4) {
            out.push(c);
        }

        // Push with a real army, then keep the pressure on.
        let combat = combat_unit_ids(g, p).len();
        let ready = combat >= 10;
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

/// Preferred build tile for an expansion refinery: just outside the nearest
/// ore field that is farther than 8 tiles from the HQ.
fn expansion_preferred(g: &Game, p: Player) -> Option<(u8, u8)> {
    let hq = g.hq(p)?;
    let hq_pos = hq.pos();
    let mut best: Option<(i64, (u8, u8))> = None;
    for idx in 0..(64 * 64) {
        if g.map.ore[idx] <= 0 {
            continue;
        }
        let (x, y) = (idx % 64, idx / 64);
        let pos = Pos::from_tile(x as u8, y as u8);
        let d = pos.dist2(&hq_pos);
        // Farther than ~8 tiles counts as an expansion field.
        if d < (8 * 256) * (8 * 256) {
            continue;
        }
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, (x as u8, y as u8)));
        }
    }
    // Build just off the ore tile (its own tile has ore and is invalid).
    best.map(|(_, (x, y))| {
        (
            (x as i32 + 1).clamp(0, 63) as u8,
            (y as i32 + 1).clamp(0, 63) as u8,
        )
    })
}

/// Public constructor.
pub fn hard() -> HardBot {
    HardBot::default()
}
