//! Economy: harvesters mine ore fields and haul it to refineries. Income comes
//! *only* from deposits — refineries give no passive trickle. A full harvester
//! walks to its refinery's dock (the tile at the refinery's front), parks
//! there for `DEPOSIT_PARK_TICKS` (1.5 s), then unloads and returns to mining.
//! Deterministic: nearest-neighbor choices break ties by lowest entity id /
//! tile index. Harvesters honor manual move orders and flee from nearby
//! enemies.

use crate::entity::{
    unit_stats, BuildingType, EntityId, UnitOrder, UnitType, DEPOSIT_PARK_TICKS,
    HARVESTER_CAPACITY, HARVEST_RATE_PER_TICK,
};
use crate::fixed::{dist2, Pos, FIX_SCALE};
use crate::game::Game;
use crate::map::MAP_TILES;
use crate::movement::step_towards;

/// Fallback deposit radius when a refinery has no passable dock tile: the
/// harvester must be adjacent to the refinery building itself — ore is
/// physically loaded into the refinery, never transferred from a distance
/// (this fallback used to be 2 tiles, which read as "dumping from across the
/// base").
const DROP_RADIUS: i32 = FIX_SCALE;
/// Distance from the dock center within which a harvester counts as docked.
/// 1.5 tiles gives a small fleet room to park at the front simultaneously
/// (units separate at 0.5 tiles), so the dock doesn't become a queue — but
/// unload still happens at the hopper, never from a distance.
const DOCK_RADIUS: i32 = FIX_SCALE * 3 / 2;
const MINE_RADIUS: i32 = FIX_SCALE * 3 / 4;
/// Radius at which harvesters flee from enemy units. Deliberately tight (2
/// tiles): income comes only from deposits, so a wide flee radius would let
/// any nearby army shut down a base's entire economy.
const FLEE_RADIUS: i32 = FIX_SCALE * 2;

impl Game {
    /// Advance the economy for one tick.
    pub fn economy_phase(&mut self) {
        let ids: Vec<EntityId> = self
            .units
            .iter()
            .filter(|u| u.is_alive() && u.utype == UnitType::Harvester)
            .map(|u| u.id)
            .collect();
        let blocked = self.blocked_grid();

        for uid in ids {
            self.harvester_tick(uid, &blocked);
        }
    }

    /// The tile a harvester docks at to unload at `rid`: the refinery's front
    /// (south side, matching the hopper sprite), falling back to the other
    /// cardinal neighbors in a fixed order. `None` if every adjacent tile is
    /// blocked (caller falls back to depositing from range).
    fn refinery_dock(&self, rid: EntityId, from_pos: Pos) -> Option<(u8, u8)> {
        let b = self.any_building(rid)?;
        let (x, y) = (b.tile.0 as i32, b.tile.1 as i32);
        let mut best: Option<(i64, (u8, u8))> = None;
        for (dx, dy) in [(0, 1), (1, 0), (-1, 0), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= 64 || ny >= 64 {
                continue;
            }
            if self.map.is_passable(nx as u8, ny as u8) {
                let tc = Pos::from_tile(nx as u8, ny as u8);
                let d = dist2(from_pos.x, from_pos.y, tc.x, tc.y);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, (nx as u8, ny as u8)));
                }
            }
        }
        best.map(|(_, t)| t)
    }

    fn harvester_tick(&mut self, uid: EntityId, blocked: &[bool]) {
        let Some(idx) = self.units.iter().position(|u| u.id == uid) else {
            return;
        };
        let owner = self.units[idx].owner;
        let pos = self.units[idx].pos;
        let carrying = self.units[idx].carrying;
        let order = self.units[idx].order.clone();
        let harvest_tile = self.units[idx].harvest_tile;
        let refinery = self.units[idx].refinery;
        let park_ticks = self.units[idx].park_ticks;
        let speed = unit_stats(UnitType::Harvester).speed;

        // Flee check: any enemy unit nearby?
        let threat = self.units.iter().any(|u| {
            u.owner != owner
                && u.is_alive()
                && dist2(pos.x, pos.y, u.pos.x, u.pos.y)
                    <= (FLEE_RADIUS as i64) * (FLEE_RADIUS as i64)
        });

        let mut new_pos = pos;
        let mut new_carrying = carrying;
        let mut new_path = self.units[idx].path.clone();
        let mut new_harvest_tile = harvest_tile;
        let mut new_refinery = refinery;
        let mut new_order = order.clone();
        let mut new_park = self.units[idx].park_ticks;
        let new_fleeing;
        let mut mined: Option<(u8, u8, i32)> = None;
        let mut deposit = 0i32;

        if threat {
            new_fleeing = true;
            let dest = self.flee_dest(owner, pos);
            let (p, _) = follow(&self.map, blocked, &mut new_path, pos, dest, speed);
            new_pos = p;
            new_park = 0;
            new_order = UnitOrder::Flee { dest };
        } else if let UnitOrder::Move { waypoint, .. } = order {
            // Manual move: honor it, then resume harvesting.
            new_fleeing = false;
            let (p, _) = follow(&self.map, blocked, &mut new_path, pos, waypoint, speed);
            new_pos = p;
            if new_path.is_empty() && new_pos.tile() == waypoint.tile() {
                new_order = UnitOrder::Idle;
            }
        } else {
            new_fleeing = false;
            if carrying >= HARVESTER_CAPACITY {
                let target = self.pick_refinery(uid, refinery);
                new_refinery = target;
                if let Some(rid) = target {
                    if let Some(b) = self.any_building(rid) {
                        if let Some(dock) = self.refinery_dock(rid, pos) {
                            let dpos = Pos::from_tile(dock.0, dock.1);
                            if dist2(pos.x, pos.y, dpos.x, dpos.y)
                                <= (DOCK_RADIUS as i64) * (DOCK_RADIUS as i64)
                            {
                                // Docked on the refinery's front tile: park,
                                // then unload and go back to mining.
                                if park_ticks >= DEPOSIT_PARK_TICKS {
                                    deposit = carrying;
                                    new_carrying = 0;
                                    new_harvest_tile = None;
                                    new_park = 0;
                                } else {
                                    new_park = park_ticks + 1;
                                }
                            } else {
                                new_park = 0;
                                let (p, _) =
                                    follow(&self.map, blocked, &mut new_path, pos, dpos, speed);
                                new_pos = p;
                            }
                        } else {
                            // No passable dock: deposit only if adjacent to the
                            // refinery itself (never from a distance).
                            let bpos = b.pos();
                            if dist2(pos.x, pos.y, bpos.x, bpos.y)
                                <= (DROP_RADIUS as i64) * (DROP_RADIUS as i64)
                            {
                                deposit = carrying;
                                new_carrying = 0;
                                new_harvest_tile = None;
                            } else {
                                let (p, _) =
                                    follow(&self.map, blocked, &mut new_path, pos, bpos, speed);
                                new_pos = p;
                            }
                        }
                    }
                }
            } else {
                let tile = harvest_tile.filter(|t| self.map.ore_at(t.0, t.1) > 0);
                let tile = tile.or_else(|| self.pick_ore_tile(pos));
                new_harvest_tile = tile;
                if let Some(t) = tile {
                    let tc = Pos::from_tile(t.0, t.1);
                    if dist2(pos.x, pos.y, tc.x, tc.y)
                        <= (MINE_RADIUS as i64) * (MINE_RADIUS as i64)
                    {
                        let space = HARVESTER_CAPACITY - carrying;
                        let take = HARVEST_RATE_PER_TICK.min(space);
                        let taken = take.min(self.map.ore_at(t.0, t.1));
                        if taken > 0 {
                            mined = Some((t.0, t.1, taken));
                            new_carrying += taken;
                        }
                    } else {
                        let (p, _) = follow(&self.map, blocked, &mut new_path, pos, tc, speed);
                        new_pos = p;
                    }
                }
            }
        }

        self.units[idx].pos = new_pos;
        self.units[idx].carrying = new_carrying;
        self.units[idx].path = new_path;
        self.units[idx].harvest_tile = new_harvest_tile;
        self.units[idx].refinery = new_refinery;
        self.units[idx].order = new_order;
        self.units[idx].fleeing = new_fleeing;
        self.units[idx].park_ticks = new_park;

        if let Some((x, y, amount)) = mined {
            self.map.deplete_ore(x, y, amount);
        }
        if deposit > 0 {
            let owner = self.units[idx].owner;
            self.ore[owner.index()] += deposit;
            self.push_event(crate::game::EventKind::OreDeposited {
                player: owner,
                amount: deposit,
            });
        }
    }

    fn pick_refinery(&self, uid: EntityId, current: Option<EntityId>) -> Option<EntityId> {
        let me = self.units.iter().find(|u| u.id == uid)?;
        let owner = me.owner;
        let pos = me.pos;
        if let Some(rid) = current {
            if let Some(b) = self.any_building(rid) {
                if b.owner == owner && b.btype == BuildingType::Refinery && b.is_alive() {
                    return Some(rid);
                }
            }
        }
        let mut best: Option<(i64, EntityId)> = None;
        for b in &self.buildings {
            if b.owner == owner && b.btype == BuildingType::Refinery && b.is_alive() {
                let d = dist2(pos.x, pos.y, b.pos().x, b.pos().y);
                if best.is_none_or(|(bd, bid)| d < bd || (d == bd && b.id < bid)) {
                    best = Some((d, b.id));
                }
            }
        }
        best.map(|(_, id)| id)
    }

    fn pick_ore_tile(&self, pos: Pos) -> Option<(u8, u8)> {
        let mut best: Option<(i64, usize)> = None;
        for idx in 0..MAP_TILES {
            if self.map.ore[idx] > 0 {
                let (x, y) = crate::map::tile_coords(idx);
                let tc = Pos::from_tile(x, y);
                let d = dist2(pos.x, pos.y, tc.x, tc.y);
                if best.is_none_or(|(bd, bidx)| d < bd || (d == bd && idx < bidx)) {
                    best = Some((d, idx));
                }
            }
        }
        best.map(|(_, idx)| crate::map::tile_coords(idx))
    }
}

/// Follow `path` one step toward `dest` (recomputing if empty).
fn follow(
    map: &crate::map::Map,
    blocked: &[bool],
    path: &mut Vec<(u8, u8)>,
    pos: Pos,
    dest: Pos,
    speed: i32,
) -> (Pos, bool) {
    if path.is_empty() {
        // Harvesters are ground units: building blockers apply.
        if let Some(p) = map.find_path(pos.tile(), dest.tile(), blocked, false) {
            *path = p;
        }
    }
    let Some(next) = path.first().copied() else {
        return (step_towards(map, blocked, pos, dest, speed, false).0, true);
    };
    let ndest = Pos::from_tile(next.0, next.1);
    let (p, arrived) = step_towards(map, blocked, pos, ndest, speed, false);
    if arrived || p.tile() == next {
        path.remove(0);
    }
    (p, false)
}
