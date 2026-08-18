//! Fog of war: per-player visibility and last-seen memory.
//!
//! [`FogView`] is the *only* object the AI commander receives. It carries
//! remembered enemy positions with their last-seen tick (so staleness can be
//! applied in `features.rs`), never the live state of hidden entities.

use serde::{Deserialize, Serialize};

use crate::entity::{BuildingType, EntityId, Player, UnitType};
use crate::fixed::{dist2, Fix, Pos, FIX_SCALE};
use crate::game::Game;
use crate::map::{tile_coords, tile_index, MAP_TILES};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RememberedUnit {
    pub id: EntityId,
    pub pos: Pos,
    pub last_seen: i32,
    pub utype: UnitType,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RememberedBuilding {
    pub id: EntityId,
    pub tile: (u8, u8),
    pub last_seen: i32,
    pub btype: BuildingType,
}

/// Per-player fog memory, part of serialized game state.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FogMemory {
    pub units: Vec<RememberedUnit>,
    pub buildings: Vec<RememberedBuilding>,
    /// Ore tiles this player has ever seen (fields don't move, but must be scouted).
    pub known_ore: Vec<bool>,
}

impl Default for FogMemory {
    fn default() -> Self {
        FogMemory {
            units: Vec::new(),
            buildings: Vec::new(),
            known_ore: vec![false; MAP_TILES],
        }
    }
}

/// A player's legal observation of the world at one tick.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FogView {
    pub player: Player,
    /// Tiles currently visible.
    pub visible: Vec<bool>,
    /// Enemy units, merged with current positions where visible.
    pub units: Vec<RememberedUnit>,
    /// Enemy buildings, merged with current positions where visible.
    pub buildings: Vec<RememberedBuilding>,
    pub known_ore: Vec<bool>,
}

impl Game {
    /// Update fog memory for both players. Call once per tick, after movement
    /// and combat (so freshly-killed units drop out and new positions stick).
    pub fn fog_phase(&mut self) {
        for player in Player::ALL {
            let visible = self.compute_visible(player);
            self.update_memory(player, &visible);
        }
    }

    /// Build the legal observation for a player.
    pub fn fog_view(&self, player: Player) -> FogView {
        let visible = self.compute_visible(player);
        let mem = &self.fog[player.index()];

        let mut units: Vec<RememberedUnit> = mem.units.clone();
        // Upsert live positions for anything visible now.
        for u in &self.units {
            if u.owner == player.enemy() && visible[tile_index(u.pos.tile().0, u.pos.tile().1)] {
                if let Some(m) = units.iter_mut().find(|m| m.id == u.id) {
                    m.pos = u.pos;
                    m.last_seen = self.tick;
                } else {
                    units.push(RememberedUnit {
                        id: u.id,
                        pos: u.pos,
                        last_seen: self.tick,
                        utype: u.utype,
                    });
                }
            }
        }

        let mut buildings: Vec<RememberedBuilding> = mem.buildings.clone();
        for b in &self.buildings {
            if b.owner == player.enemy() && visible[tile_index(b.tile.0, b.tile.1)] {
                if let Some(m) = buildings.iter_mut().find(|m| m.id == b.id) {
                    m.tile = b.tile;
                    m.last_seen = self.tick;
                } else {
                    buildings.push(RememberedBuilding {
                        id: b.id,
                        tile: b.tile,
                        last_seen: self.tick,
                        btype: b.btype,
                    });
                }
            }
        }

        FogView {
            player,
            visible,
            units,
            buildings,
            known_ore: mem.known_ore.clone(),
        }
    }

    fn compute_visible(&self, player: Player) -> Vec<bool> {
        let mut visible = vec![false; MAP_TILES];
        for u in &self.units {
            if u.owner == player && u.is_alive() {
                let vision = crate::entity::unit_stats(u.utype).vision;
                mark_visible(&mut visible, u.pos, vision);
            }
        }
        for b in &self.buildings {
            if b.owner == player && b.is_alive() {
                let vision = crate::entity::building_stats(b.btype).vision;
                mark_visible(&mut visible, b.pos(), vision);
            }
        }
        visible
    }

    fn update_memory(&mut self, player: Player, visible: &[bool]) {
        let enemy = player.enemy();
        let tick = self.tick;

        // Units: refresh visible, drop long-dead.
        let mut units: Vec<RememberedUnit> = Vec::new();
        for u in &self.units {
            if u.owner == enemy && visible[tile_index(u.pos.tile().0, u.pos.tile().1)] {
                units.push(RememberedUnit {
                    id: u.id,
                    pos: u.pos,
                    last_seen: tick,
                    utype: u.utype,
                });
            }
        }
        // Carry forward remembered units that are still alive but not visible.
        let live_ids: Vec<EntityId> = self.units.iter().map(|u| u.id).collect();
        for m in &self.fog[player.index()].units {
            if live_ids.contains(&m.id)
                && !units.iter().any(|x| x.id == m.id)
                && m.last_seen >= tick - crate::fixed::TICKS_PER_SEC * 60
            {
                units.push(m.clone());
            }
        }
        self.fog[player.index()].units = units;

        // Buildings: remember visible ones, retain remembered-but-alive.
        let mut buildings: Vec<RememberedBuilding> = Vec::new();
        for b in &self.buildings {
            if b.owner == enemy && visible[tile_index(b.tile.0, b.tile.1)] {
                buildings.push(RememberedBuilding {
                    id: b.id,
                    tile: b.tile,
                    last_seen: tick,
                    btype: b.btype,
                });
            }
        }
        let live_bids: Vec<EntityId> = self.buildings.iter().map(|b| b.id).collect();
        for m in &self.fog[player.index()].buildings {
            if live_bids.contains(&m.id) && !buildings.iter().any(|x| x.id == m.id) {
                buildings.push(m.clone());
            }
        }
        self.fog[player.index()].buildings = buildings;

        // Known ore: union with currently visible ore tiles.
        for (idx, &seen) in visible.iter().enumerate() {
            if seen && self.map.ore[idx] > 0 {
                self.fog[player.index()].known_ore[idx] = true;
            }
        }
    }
}

/// Mark all tiles within `vision` fix units of `pos` as visible.
fn mark_visible(visible: &mut [bool], pos: Pos, vision: Fix) {
    let radius_tiles = vision / FIX_SCALE + 1;
    let cx = pos.x / FIX_SCALE;
    let cy = pos.y / FIX_SCALE;
    let v2 = (vision as i64) * (vision as i64);
    for ty in (cy - radius_tiles).max(0)..=(cy + radius_tiles).min(63) {
        for tx in (cx - radius_tiles).max(0)..=(cx + radius_tiles).min(63) {
            let tc = Pos::from_tile(tx as u8, ty as u8);
            if dist2(pos.x, pos.y, tc.x, tc.y) <= v2 {
                visible[tile_index(tx as u8, ty as u8)] = true;
            }
        }
    }
}

#[allow(dead_code)]
fn _tile_helpers() -> ((u8, u8), usize) {
    (tile_coords(0), 0)
}
