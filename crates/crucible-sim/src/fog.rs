//! Fog of war: per-player visibility and last-seen memory.
//!
//! [`FogView`] is the *only* object the AI commander receives. It carries
//! remembered enemy positions with their last-seen tick (so staleness can be
//! applied in `features.rs`), never the live state of hidden entities.

use serde::{Deserialize, Serialize};

use crate::entity::{BuildingType, EntityId, Player, UnitType};
use crate::fixed::{dist2, Fix, Pos, FIX_SCALE};
use crate::game::Game;
use crate::map::{tile_index, MAP_TILES};

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
    /// Every tile this player has ever seen (monotonic; powers the AI's
    /// "unexplored fraction" observation). `#[serde(default)]` keeps old
    /// persisted states loadable — a missing field starts fully unexplored.
    #[serde(default)]
    pub explored: Vec<bool>,
}

impl Default for FogMemory {
    fn default() -> Self {
        FogMemory {
            units: Vec::new(),
            buildings: Vec::new(),
            known_ore: vec![false; MAP_TILES],
            explored: vec![false; MAP_TILES],
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
    /// Every tile this player has ever seen.
    pub explored: Vec<bool>,
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
            explored: mem.explored.clone(),
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
        // Carry forward hidden sightings without consulting authoritative enemy
        // state. A player may forget a unit only after the memory timeout, or
        // after re-observing its last-known tile and seeing it is gone.
        for m in &self.fog[player.index()].units {
            let last_tile = m.pos.tile();
            if !visible[tile_index(last_tile.0, last_tile.1)]
                && !units.iter().any(|x| x.id == m.id)
                && m.last_seen >= tick - crate::fixed::TICKS_PER_SEC * 60
            {
                units.push(m.clone());
            }
        }
        self.fog[player.index()].units = units;

        // Buildings follow the same rule as units: do not reveal an unseen
        // destruction by consulting live state behind the fog.
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
        for m in &self.fog[player.index()].buildings {
            if !visible[tile_index(m.tile.0, m.tile.1)]
                && !buildings.iter().any(|x| x.id == m.id)
                && m.last_seen >= tick - crate::fixed::TICKS_PER_SEC * 60
            {
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

        // Explored: every tile ever seen (monotonic union).
        let explored = &mut self.fog[player.index()].explored;
        for (idx, &seen) in visible.iter().enumerate() {
            if seen {
                explored[idx] = true;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Game, GameConfig, Map, Player};

    #[test]
    fn hidden_destruction_does_not_remove_last_seen_memory() {
        let mut game = Game::new(Map::generate(1), GameConfig::default());
        let unit_pos = Pos::from_tile(30, 30);
        game.fog[Player::P0.index()].units.push(RememberedUnit {
            id: 999,
            pos: unit_pos,
            last_seen: 0,
            utype: UnitType::Infantry,
        });
        game.fog[Player::P0.index()]
            .buildings
            .push(RememberedBuilding {
                id: 1_000,
                tile: (31, 31),
                last_seen: 0,
                btype: BuildingType::Factory,
            });

        // Neither remembered tile is visible, and the entities do not exist
        // in authoritative state. Their absence must not leak through fog.
        game.update_memory(Player::P0, &vec![false; MAP_TILES]);
        assert_eq!(game.fog[0].units.len(), 1);
        assert_eq!(game.fog[0].buildings.len(), 1);

        // Re-observing the remembered locations is enough to remove them.
        let mut visible = vec![false; MAP_TILES];
        visible[tile_index(30, 30)] = true;
        visible[tile_index(31, 31)] = true;
        game.update_memory(Player::P0, &visible);
        assert!(game.fog[0].units.is_empty());
        assert!(game.fog[0].buildings.is_empty());
    }
}
