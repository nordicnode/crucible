//! The fixed-step advance: one call to [`Game::step`] moves game time
//! forward exactly one tick (100ms). Phase order is part of the determinism
//! contract.

use crate::entity::{unit_stats, Player, Stance, Unit, UnitOrder, UnitType, Upgrade};
use crate::fixed::{isqrt, Pos, FIX_SCALE};
use crate::game::Game;

const SPAWN_NEIGHBORS: [(i8, i8); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

impl Game {
    /// Advance one sim tick. No-op if the match is already over.
    pub fn step(&mut self) {
        if self.is_over() {
            return;
        }
        self.tick += 1;
        self.apm[0].tick();
        self.apm[1].tick();

        let low_power = [
            self.has_low_power(Player::P0),
            self.has_low_power(Player::P1),
        ];

        // Cooldowns tick down.
        for u in &mut self.units {
            if u.cooldown > 0 {
                u.cooldown -= 1;
            }
        }
        for b in &mut self.buildings {
            if b.cooldown > 0 {
                // Low power penalty: turret cooldown recovers at 50% rate
                if low_power[b.owner.index()] && self.tick % 2 != 0 {
                    continue;
                }
                b.cooldown -= 1;
            }
        }

        self.economy_phase();
        self.production_phase();
        self.combat_phase();
        self.turret_phase();
        self.separation_phase();
        self.sweep_dead();
        self.fog_phase();
        self.check_win();
    }

    /// Push overlapping units apart so they don't stack on one spot.
    ///
    /// Deterministic: pairs are considered in ascending id order and only the
    /// later id moves, so the result does not depend on iteration order. The
    /// pushed position is only applied when its tile is passable and not
    /// occupied by a building.
    fn separation_phase(&mut self) {
        const MIN_SEP: i64 = FIX_SCALE as i64 / 2; // 0.5 tile
        const MIN_SEP2: i64 = MIN_SEP * MIN_SEP;
        let blocked = self.blocked_grid();
        let n = self.units.len();
        for i in 0..n {
            let a = self.units[i].pos;
            for j in (i + 1)..n {
                let b = self.units[j].pos;
                let dx = b.x as i64 - a.x as i64;
                let dy = b.y as i64 - a.y as i64;
                let d2 = dx * dx + dy * dy;
                if d2 >= MIN_SEP2 {
                    continue;
                }
                let (nx, ny) = if d2 == 0 {
                    // Exact stack: fall back to +x.
                    (b.x + MIN_SEP as i32, b.y)
                } else {
                    let d = isqrt(d2);
                    let push = ((MIN_SEP - d) / 2).max(1);
                    (b.x + (dx * push / d) as i32, b.y + (dy * push / d) as i32)
                };
                let (tx, ty) = (crate::fixed::fix_to_tile(nx), crate::fixed::fix_to_tile(ny));
                if tx < 64
                    && ty < 64
                    && self.map.is_passable(tx, ty)
                    && !blocked[crate::map::tile_index(tx, ty)]
                {
                    self.units[j].pos = Pos::new(nx, ny);
                }
            }
        }
    }

    /// Advance production queues and spawn completed units.
    fn production_phase(&mut self) {
        type Spawn = (Player, UnitType, (u8, u8), Option<(u8, u8)>);
        let mut spawns: Vec<Spawn> = Vec::new();
        let low_power = [
            self.has_low_power(Player::P0),
            self.has_low_power(Player::P1),
        ];

        for i in 0..self.buildings.len() {
            if self.buildings[i].queue.is_empty() {
                self.buildings[i].progress = 0;
                continue;
            }
            let owner = self.buildings[i].owner;
            // Low power penalty: unit production progresses at 50% speed
            if low_power[owner.index()] && self.tick % 2 != 0 {
                continue;
            }
            let utype = self.buildings[i].queue[0];
            let build_time = unit_stats(utype).build_time;
            self.buildings[i].progress += 1;
            if self.buildings[i].progress >= build_time {
                let owner = self.buildings[i].owner;
                let tile = self.buildings[i].tile;
                let rally = self.buildings[i].rally;
                if let Some(st) = self.pick_spawn_tile(tile) {
                    spawns.push((owner, utype, st, rally));
                    self.buildings[i].queue.remove(0);
                    self.buildings[i].progress = 0;
                } else {
                    // Wait at completion until a spawn tile frees up.
                    self.buildings[i].progress = build_time - 1;
                }
            }
        }

        for (owner, utype, tile, rally) in spawns {
            self.spawn_unit(owner, utype, tile, rally);
        }
    }

    pub(crate) fn pick_spawn_tile(&self, building: (u8, u8)) -> Option<(u8, u8)> {
        for (dx, dy) in SPAWN_NEIGHBORS {
            let x = building.0 as i32 + dx as i32;
            let y = building.1 as i32 + dy as i32;
            if x < 0 || y < 0 || x >= 64 || y >= 64 {
                continue;
            }
            let t = (x as u8, y as u8);
            if self.map.is_passable(t.0, t.1) && self.building_at(t).is_none() {
                return Some(t);
            }
        }
        None
    }

    pub(crate) fn spawn_unit(
        &mut self,
        owner: Player,
        utype: UnitType,
        tile: (u8, u8),
        rally: Option<(u8, u8)>,
    ) {
        let stats = unit_stats(utype);
        let mut hp = stats.hp;
        if self.upgrades[owner.index()] == Upgrade::Hp {
            hp = hp * 115 / 100;
        }
        let id = self.alloc_id();
        let pos = Pos::from_tile(tile.0, tile.1);

        let order = if utype == UnitType::Harvester {
            UnitOrder::Harvest
        } else if let Some(r) = rally {
            UnitOrder::Move {
                waypoint: Pos::from_tile(r.0, r.1),
                stance: Stance::Aggressive,
            }
        } else {
            UnitOrder::Idle
        };
        let path = if let UnitOrder::Move { waypoint, .. } = order {
            self.map
                .find_path((tile.0, tile.1), waypoint.tile(), &self.blocked_grid())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        self.units.push(Unit {
            id,
            owner,
            utype,
            pos,
            hp,
            max_hp: hp,
            stance: Stance::Aggressive,
            order,
            carrying: 0,
            cooldown: 0,
            park_ticks: 0,
            path,
            target: None,
            fleeing: false,
            harvest_tile: None,
            refinery: None,
        });
        self.push_event(crate::game::EventKind::UnitTrained {
            player: owner,
            utype,
            tile,
        });
    }

    /// Remove dead units/buildings (ascending id order is preserved by retain).
    fn sweep_dead(&mut self) {
        self.units.retain(|u| u.is_alive());
        self.buildings.retain(|b| b.is_alive());
    }
}
