//! Deterministic combat: target acquisition, stance behavior, movement
//! (chase / keep-range / flee / waypoint), and damage with tank splash.
//!
//! Units are processed in ascending entity id order every tick; damage is
//! applied immediately so target choice (lowest-HP) is well-defined and
//! reproducible. Both enemy units and buildings are valid targets.

use crate::entity::{
    unit_stats, EntityId, Player, Stance, UnitOrder, Upgrade, RETREAT_HP_FRACTION_DEN,
    RETREAT_HP_FRACTION_NUM,
};
use crate::fixed::{dist2, Pos};
use crate::game::Game;
use crate::movement::step_towards;

/// A damageable enemy entity (unit or building).
#[derive(Clone, Copy)]
struct Target {
    id: EntityId,
    pos: Pos,
    hp: i32,
    building: bool,
}

impl Game {
    /// Combat and combat-unit movement for one tick.
    pub fn combat_phase(&mut self) {
        let ids: Vec<EntityId> = self
            .units
            .iter()
            .filter(|u| u.is_alive() && unit_stats(u.utype).damage > 0)
            .map(|u| u.id)
            .collect();
        let blocked = self.blocked_grid();
        for uid in ids {
            self.combat_unit_tick(uid, &blocked);
        }
    }

    /// Turret firing (buildings that attack).
    pub fn turret_phase(&mut self) {
        let ids: Vec<EntityId> = self
            .buildings
            .iter()
            .filter(|b| b.is_alive() && crate::entity::building_stats(b.btype).damage > 0)
            .map(|b| b.id)
            .collect();
        for bid in ids {
            self.turret_tick(bid);
        }
    }

    fn enemies_of(&self, enemy: Player) -> Vec<Target> {
        let mut out: Vec<Target> = self
            .units
            .iter()
            .filter(|u| u.owner == enemy && u.is_alive())
            .map(|u| Target {
                id: u.id,
                pos: u.pos,
                hp: u.hp,
                building: false,
            })
            .collect();
        out.extend(
            self.buildings
                .iter()
                .filter(|b| b.owner == enemy && b.is_alive())
                .map(|b| Target {
                    id: b.id,
                    pos: b.pos(),
                    hp: b.hp,
                    building: true,
                }),
        );
        out
    }

    fn combat_unit_tick(&mut self, uid: EntityId, blocked: &[bool]) {
        let Some(idx) = self.units.iter().position(|u| u.id == uid) else {
            return;
        };
        let utype = self.units[idx].utype;
        let stats = unit_stats(utype);
        let owner = self.units[idx].owner;
        let enemy = owner.enemy();
        let pos = self.units[idx].pos;
        let stance = self.units[idx].stance;
        let order = self.units[idx].order.clone();
        let hp = self.units[idx].hp;
        let max_hp = self.units[idx].max_hp;
        let cooldown = self.units[idx].cooldown;
        let mut path = self.units[idx].path.clone();
        let mut target = self.units[idx].target;

        let enemies = self.enemies_of(enemy);

        let vision2 = (stats.vision as i64) * (stats.vision as i64);
        let range2 = (stats.range as i64) * (stats.range as i64);
        let min_range2 = (stats.min_range as i64) * (stats.min_range as i64);

        if let Some(t) = target {
            let valid = enemies
                .iter()
                .any(|e| e.id == t && dist2(pos.x, pos.y, e.pos.x, e.pos.y) <= vision2);
            if !valid {
                target = None;
            }
        }

        let low_hp = hp * RETREAT_HP_FRACTION_DEN < max_hp * RETREAT_HP_FRACTION_NUM;
        let should_flee = stance == Stance::Cautious && low_hp;

        let mut new_pos = pos;
        let mut new_target = target;
        let mut damage: Vec<(EntityId, bool, i32)> = Vec::new(); // (id, is_building, amount)
        let mut new_cooldown = cooldown;

        let new_fleeing = if should_flee {
            let dest = self.flee_dest(owner, pos);
            new_pos = self.move_along_path(blocked, &mut path, pos, dest, stats.speed);
            true
        } else {
            if new_target.is_none() {
                let in_range = enemies
                    .iter()
                    .filter(|e| {
                        let d2 = dist2(pos.x, pos.y, e.pos.x, e.pos.y);
                        d2 <= range2 && d2 >= min_range2
                    })
                    .min_by(|a, b| a.hp.cmp(&b.hp).then_with(|| a.id.cmp(&b.id)));
                if let Some(e) = in_range {
                    new_target = Some(e.id);
                } else if stance != Stance::Hold {
                    let nearest = enemies
                        .iter()
                        .filter(|e| dist2(pos.x, pos.y, e.pos.x, e.pos.y) <= vision2)
                        .min_by(|a, b| {
                            dist2(pos.x, pos.y, a.pos.x, a.pos.y)
                                .cmp(&dist2(pos.x, pos.y, b.pos.x, b.pos.y))
                                .then_with(|| a.id.cmp(&b.id))
                        });
                    if let Some(e) = nearest {
                        new_target = Some(e.id);
                    }
                }
            }

            if let Some(t) = new_target {
                if let Some(tpos) = enemies.iter().find(|e| e.id == t).map(|e| e.pos) {
                    let d2 = dist2(pos.x, pos.y, tpos.x, tpos.y);
                    let in_fire_range = d2 <= range2 && d2 >= min_range2;
                    let can_chase = stance != Stance::Hold;

                    if stats.min_range > 0 && d2 < min_range2 {
                        if can_chase {
                            new_pos = step_away(&self.map, blocked, pos, tpos, stats.speed);
                        }
                    } else if !in_fire_range && can_chase {
                        new_pos = step_direct(&self.map, blocked, pos, tpos, stats.speed);
                    }

                    if in_fire_range && new_cooldown <= 0 {
                        let dmg = apply_damage_upgrade(stats.damage, self.upgrades[owner.index()]);
                        if let Some(te) = enemies.iter().find(|e| e.id == t) {
                            damage.push((t, te.building, dmg));
                        }
                        if stats.splash > 0 {
                            let splash2 = (stats.splash as i64) * (stats.splash as i64);
                            for e in &enemies {
                                if e.id != t && dist2(tpos.x, tpos.y, e.pos.x, e.pos.y) <= splash2 {
                                    damage.push((e.id, e.building, dmg));
                                }
                            }
                        }
                        new_cooldown = stats.cooldown;
                    }
                }
            } else if let UnitOrder::Move { waypoint, .. } = order {
                new_pos = self.move_along_path(blocked, &mut path, pos, waypoint, stats.speed);
            }
            false
        };

        self.units[idx].pos = new_pos;
        self.units[idx].target = new_target;
        self.units[idx].fleeing = new_fleeing;
        self.units[idx].cooldown = new_cooldown.max(0);
        self.units[idx].path = path;

        for (victim, is_building, amount) in damage {
            self.apply_damage(victim, is_building, amount);
        }
    }

    fn turret_tick(&mut self, bid: EntityId) {
        let Some(idx) = self.buildings.iter().position(|b| b.id == bid) else {
            return;
        };
        let stats = crate::entity::building_stats(self.buildings[idx].btype);
        let owner = self.buildings[idx].owner;
        let enemy = owner.enemy();
        let pos = self.buildings[idx].pos();
        let cooldown = self.buildings[idx].cooldown;

        let range2 = (stats.range as i64) * (stats.range as i64);
        let mut fired = false;

        let enemies = self.enemies_of(enemy);
        let target = enemies
            .iter()
            .filter(|e| dist2(pos.x, pos.y, e.pos.x, e.pos.y) <= range2)
            .min_by(|a, b| a.hp.cmp(&b.hp).then_with(|| a.id.cmp(&b.id)))
            .map(|e| (e.id, e.building));

        if let Some((t, is_bld)) = target {
            if cooldown <= 0 {
                let dmg = apply_damage_upgrade(stats.damage, self.upgrades[owner.index()]);
                fired = true;
                self.apply_damage(t, is_bld, dmg);
            }
        }
        if fired {
            self.buildings[idx].cooldown = stats.cooldown;
        }
    }

    /// Apply damage to a unit or building; records the death event.
    fn apply_damage(&mut self, id: EntityId, is_building: bool, amount: i32) {
        if is_building {
            if let Some(bi) = self.buildings.iter().position(|b| b.id == id) {
                let was_alive = self.buildings[bi].is_alive();
                self.buildings[bi].hp -= amount;
                if was_alive && !self.buildings[bi].is_alive() {
                    let owner = self.buildings[bi].owner;
                    self.push_event(crate::game::EventKind::BuildingDestroyed { id, owner });
                }
            }
        } else if let Some(ui) = self.units.iter().position(|u| u.id == id) {
            let was_alive = self.units[ui].is_alive();
            self.units[ui].hp -= amount;
            if was_alive && !self.units[ui].is_alive() {
                let owner = self.units[ui].owner;
                self.push_event(crate::game::EventKind::UnitDied { id, owner });
            }
        }
    }

    fn move_along_path(
        &mut self,
        blocked: &[bool],
        path: &mut Vec<(u8, u8)>,
        pos: Pos,
        dest: Pos,
        speed: i32,
    ) -> Pos {
        if path.is_empty() {
            if let Some(p) = self.map.find_path(pos.tile(), dest.tile(), blocked) {
                *path = p;
            }
        }
        let Some(next) = path.first().copied() else {
            return step_direct(&self.map, blocked, pos, dest, speed);
        };
        let ndest = Pos::from_tile(next.0, next.1);
        let (p, arrived) = step_towards(&self.map, blocked, pos, ndest, speed);
        if arrived || p.tile() == next {
            path.remove(0);
        }
        p
    }
}

fn apply_damage_upgrade(damage: i32, upgrade: Upgrade) -> i32 {
    match upgrade {
        Upgrade::Damage => damage * 115 / 100,
        _ => damage,
    }
}

fn step_direct(map: &crate::map::Map, blocked: &[bool], pos: Pos, dest: Pos, speed: i32) -> Pos {
    step_towards(map, blocked, pos, dest, speed).0
}

fn step_away(map: &crate::map::Map, blocked: &[bool], pos: Pos, from: Pos, speed: i32) -> Pos {
    let dx = pos.x as i64 - from.x as i64;
    let dy = pos.y as i64 - from.y as i64;
    if dx == 0 && dy == 0 {
        return pos;
    }
    let dest = Pos::new(pos.x + dx as i32, pos.y + dy as i32);
    step_towards(map, blocked, pos, dest, speed).0
}
