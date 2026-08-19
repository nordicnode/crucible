//! Match orchestration: state container, command application, APM budget,
//! win check, and helpers shared by the per-phase modules.

use serde::{Deserialize, Serialize};

use crate::entity::{
    building_stats, unit_stats, Building, BuildingType, EntityId, Player, Unit, UnitType, Upgrade,
};
use crate::fixed::{dist2, Pos, COMMAND_TICK, MATCH_TIMEOUT_TICKS, TICKS_PER_SEC};
use crate::fog::FogMemory;
use crate::map::Map;
use crate::movement::formation_tile;
use crate::orders::{Command, CommandError};

/// Runtime-tunable match settings. Tests override via a builder.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameConfig {
    pub max_queue: usize,
    /// Commands per minute budget (applies to all issuers, including the AI).
    pub apm_cap: u32,
    pub starting_ore: i32,
    /// Fraction of a building's cost refunded on sell (50/100 = 50%).
    pub sell_refund_num: i32,
    pub sell_refund_den: i32,
    pub timeout_ticks: i32,
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig {
            max_queue: 5,
            apm_cap: 120,
            starting_ore: 450,
            sell_refund_num: 1,
            sell_refund_den: 2,
            timeout_ticks: MATCH_TIMEOUT_TICKS,
        }
    }
}

/// Token-bucket APM budget (fixed-point, 1000 units = 1 command).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApmBudget {
    bucket: i64,
    cap: i64,
    refill_per_tick: i64,
}

impl ApmBudget {
    const COST: i64 = 1000;

    fn new(apm_cap: u32) -> Self {
        let cap = apm_cap as i64 * Self::COST;
        ApmBudget {
            bucket: cap,
            cap,
            refill_per_tick: cap / (60 * TICKS_PER_SEC) as i64,
        }
    }

    pub(crate) fn tick(&mut self) {
        self.bucket = (self.bucket + self.refill_per_tick).min(self.cap);
    }

    pub(crate) fn try_spend(&mut self) -> bool {
        if self.bucket >= Self::COST {
            self.bucket -= Self::COST;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum WinReason {
    HqDestroyed,
    Timeout,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum EventKind {
    BuildingPlaced {
        player: Player,
        btype: BuildingType,
        tile: (u8, u8),
    },
    UnitTrained {
        player: Player,
        utype: crate::entity::UnitType,
        tile: (u8, u8),
    },
    UnitDied {
        id: EntityId,
        owner: Player,
    },
    BuildingDestroyed {
        id: EntityId,
        owner: Player,
    },
    OreDeposited {
        player: Player,
        amount: i32,
    },
    Sold {
        player: Player,
        btype: BuildingType,
        refund: i32,
    },
    UpgradeChosen {
        player: Player,
        upgrade: Upgrade,
    },
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GameEvent {
    pub tick: i32,
    pub kind: EventKind,
}

/// The complete match state. The only mutable root of the simulation.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Game {
    pub config: GameConfig,
    pub map: Map,
    pub buildings: Vec<Building>,
    pub units: Vec<Unit>,
    pub ore: [i32; 2],
    pub upgrades: [Upgrade; 2],
    pub tick: i32,
    pub winner: Option<Player>,
    pub win_reason: Option<WinReason>,
    /// Whether the match has reached a terminal result. Kept separate from
    /// `winner` so a legitimate draw can end the match without naming a side.
    #[serde(default)]
    pub over: bool,
    pub apm: [ApmBudget; 2],
    pub next_id: EntityId,
    pub events: Vec<GameEvent>,
    /// Number of commands dropped by the APM budget, per player (debug counter).
    pub dropped_commands: [u32; 2],
    /// Per-player fog-of-war memory.
    pub fog: [FogMemory; 2],
}

impl Game {
    pub fn new(map: Map, config: GameConfig) -> Self {
        let mut next_id = 1u32;
        let mut buildings = Vec::new();
        for (p, tile) in Player::ALL.iter().zip(map.hq_tiles.iter()) {
            let stats = building_stats(BuildingType::Hq);
            buildings.push(Building {
                id: next_id,
                owner: *p,
                btype: BuildingType::Hq,
                tile: *tile,
                hp: stats.hp,
                max_hp: stats.hp,
                queue: Vec::new(),
                progress: 0,
                rally: None,
                cooldown: 0,
            });
            next_id += 1;
        }
        let ore = [config.starting_ore; 2];
        let mut g = Game {
            config: config.clone(),
            map,
            buildings,
            units: Vec::new(),
            ore,
            upgrades: [Upgrade::None; 2],
            tick: 0,
            winner: None,
            win_reason: None,
            over: false,
            apm: [
                ApmBudget::new(config.apm_cap),
                ApmBudget::new(config.apm_cap),
            ],
            next_id,
            events: Vec::new(),
            dropped_commands: [0, 0],
            fog: [FogMemory::default(), FogMemory::default()],
        };
        // Each side starts with one Harvester so the mining loop is visible
        // from the first second (spawned adjacent to the HQ).
        let hq_tiles = g.map.hq_tiles;
        for (p, tile) in Player::ALL.iter().zip(hq_tiles.iter()) {
            if let Some(t) = g.pick_spawn_tile(*tile) {
                g.spawn_unit(*p, UnitType::Harvester, t, None);
            }
        }
        g
    }

    /// Per-tile blocked overlay: tiles occupied by a building (terrain
    /// passability is separate, in [`Map::passable`]).
    pub fn blocked_grid(&self) -> Vec<bool> {
        let mut b = vec![false; crate::map::MAP_TILES];
        for x in &self.buildings {
            b[crate::map::tile_index(x.tile.0, x.tile.1)] = true;
        }
        b
    }

    pub fn alloc_id(&mut self) -> EntityId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn unit(&self, player: Player, id: EntityId) -> Option<&Unit> {
        self.units.iter().find(|u| u.id == id && u.owner == player)
    }

    pub fn unit_mut(&mut self, player: Player, id: EntityId) -> Option<&mut Unit> {
        self.units
            .iter_mut()
            .find(|u| u.id == id && u.owner == player)
    }

    pub fn building(&self, player: Player, id: EntityId) -> Option<&Building> {
        self.buildings
            .iter()
            .find(|b| b.id == id && b.owner == player)
    }

    pub fn building_mut(&mut self, player: Player, id: EntityId) -> Option<&mut Building> {
        self.buildings
            .iter_mut()
            .find(|b| b.id == id && b.owner == player)
    }

    /// Find any building (any owner) by id.
    pub fn any_building(&self, id: EntityId) -> Option<&Building> {
        self.buildings.iter().find(|b| b.id == id)
    }

    /// Find any unit (any owner) by id.
    pub fn any_unit(&self, id: EntityId) -> Option<&Unit> {
        self.units.iter().find(|u| u.id == id)
    }

    pub fn building_at(&self, tile: (u8, u8)) -> Option<EntityId> {
        self.buildings.iter().find(|b| b.tile == tile).map(|b| b.id)
    }

    /// The HQ building of a player, if alive.
    pub fn hq(&self, player: Player) -> Option<&Building> {
        self.buildings
            .iter()
            .find(|b| b.owner == player && b.btype == BuildingType::Hq)
    }

    pub fn push_event(&mut self, kind: EventKind) {
        self.events.push(GameEvent {
            tick: self.tick,
            kind,
        });
    }

    /// Nearest own HQ/refinery to flee toward (ties: lowest building id).
    pub fn flee_dest(&self, owner: Player, pos: Pos) -> Pos {
        let mut best: Option<(i64, EntityId, Pos)> = None;
        for b in &self.buildings {
            if b.owner == owner
                && (b.btype == BuildingType::Hq || b.btype == BuildingType::Refinery)
                && b.is_alive()
            {
                let d = dist2(pos.x, pos.y, b.pos().x, b.pos().y);
                let better = match best {
                    None => true,
                    Some((bd, bid, _)) => d < bd || (d == bd && b.id < bid),
                };
                if better {
                    best = Some((d, b.id, b.pos()));
                }
            }
        }
        best.map(|(_, _, p)| p).unwrap_or(pos)
    }

    /// Return (power_produced, power_consumed) for a given player.
    pub fn power(&self, player: Player) -> (i32, i32) {
        let mut produced = 0;
        let mut consumed = 0;
        for b in &self.buildings {
            if b.owner == player && b.is_alive() {
                let stats = crate::entity::building_stats(b.btype);
                if stats.power > 0 {
                    produced += stats.power;
                } else if stats.power < 0 {
                    consumed += -stats.power;
                }
            }
        }
        (produced, consumed)
    }

    /// True if a player's power consumption exceeds their power production.
    pub fn has_low_power(&self, player: Player) -> bool {
        let (prod, cons) = self.power(player);
        cons > prod
    }

    /// Validate and apply a batch of commands for one player. Returns a
    /// per-command result so the server can report exactly which were dropped
    /// and why. Commands are applied in the order given; APM budget is shared
    /// across the batch.
    pub fn apply_commands(
        &mut self,
        player: Player,
        cmds: &[Command],
    ) -> Vec<Result<(), CommandError>> {
        cmds.iter().map(|cmd| self.apply_one(player, cmd)).collect()
    }

    fn apply_one(&mut self, player: Player, cmd: &Command) -> Result<(), CommandError> {
        if cmd.player() != player {
            return Err(CommandError::NotYourEntity);
        }
        self.validate_command(cmd)?;
        if !self.apm[player.index()].try_spend() {
            self.dropped_commands[player.index()] += 1;
            return Err(CommandError::RateLimited);
        }
        self.execute(player, cmd);
        Ok(())
    }

    fn execute(&mut self, player: Player, cmd: &Command) {
        match cmd {
            Command::PlaceBuilding { btype, tile, .. } => {
                let stats = building_stats(*btype);
                self.ore[player.index()] -= stats.cost;
                let id = self.alloc_id();
                self.buildings.push(Building {
                    id,
                    owner: player,
                    btype: *btype,
                    tile: *tile,
                    hp: stats.hp,
                    max_hp: stats.hp,
                    queue: Vec::new(),
                    progress: 0,
                    rally: None,
                    cooldown: 0,
                });
                self.push_event(EventKind::BuildingPlaced {
                    player,
                    btype: *btype,
                    tile: *tile,
                });
            }
            Command::TrainUnit {
                building, utype, ..
            } => {
                let stats = unit_stats(*utype);
                self.ore[player.index()] -= stats.cost;
                if let Some(b) = self.building_mut(player, *building) {
                    b.queue.push(*utype);
                }
            }
            Command::MoveGroup {
                units,
                waypoint,
                stance,
                ..
            } => {
                // Spread the group around the waypoint (formation) so units do
                // not all converge on one tile; each unit paths to its own
                // offset destination. Deterministic by unit index.
                let blocked = self.blocked_grid();
                let mut moves: Vec<(EntityId, Vec<(u8, u8)>)> = Vec::new();
                for (i, id) in units.iter().enumerate() {
                    if let Some(u) = self.unit(player, *id) {
                        let tile = formation_tile(*waypoint, i, &self.map, &blocked);
                        let path = self
                            .map
                            .find_path(u.pos.tile(), tile, &blocked)
                            .unwrap_or_default();
                        moves.push((*id, path));
                    }
                }
                for (id, path) in moves {
                    if let Some(u) = self.unit_mut(player, id) {
                        u.stance = *stance;
                        let dest = path
                            .last()
                            .copied()
                            .map(|t| Pos::from_tile(t.0, t.1))
                            .unwrap_or_else(|| Pos::from_tile(waypoint.0, waypoint.1));
                        u.order = crate::entity::UnitOrder::Move {
                            waypoint: dest,
                            stance: *stance,
                        };
                        u.path = path;
                        u.target = None;
                        u.fleeing = false;
                    }
                }
            }
            Command::SetRally {
                building, waypoint, ..
            } => {
                if let Some(b) = self.building_mut(player, *building) {
                    b.rally = Some(*waypoint);
                }
            }
            Command::ChooseUpgrade { lab, upgrade, .. } => {
                if let Some(_b) = self.building(player, *lab) {
                    self.upgrades[player.index()] = *upgrade;
                    self.push_event(EventKind::UpgradeChosen {
                        player,
                        upgrade: *upgrade,
                    });
                }
            }
            Command::Sell { building, .. } => {
                let btype = self.building(player, *building).map(|b| b.btype);
                if let Some(bt) = btype {
                    let stats = building_stats(bt);
                    let refund =
                        stats.cost * self.config.sell_refund_num / self.config.sell_refund_den;
                    self.ore[player.index()] += refund;
                    self.push_event(EventKind::Sold {
                        player,
                        btype: bt,
                        refund,
                    });
                    let id = *building;
                    self.buildings.retain(|b| b.id != id);
                }
            }
            Command::Repair { building, .. } => {
                let cost = 15;
                if self.ore[player.index()] >= cost {
                    if let Some(pos) = self
                        .buildings
                        .iter()
                        .position(|b| b.id == *building && b.owner == player && b.is_alive())
                    {
                        if self.buildings[pos].hp < self.buildings[pos].max_hp {
                            self.ore[player.index()] -= cost;
                            let max_hp = self.buildings[pos].max_hp;
                            self.buildings[pos].hp = (self.buildings[pos].hp + 100).min(max_hp);
                        }
                    }
                }
            }
        }
    }

    /// True once the match has ended (timeout or HQ destruction).
    pub fn is_over(&self) -> bool {
        // `winner` keeps snapshots produced before the explicit draw marker
        // replayable: those snapshots ended only when a winner was present.
        self.over || self.winner.is_some()
    }

    /// The command tick index (0-based) for the current game tick.
    pub fn command_tick_index(&self) -> i32 {
        self.tick / COMMAND_TICK
    }

    /// True if the current tick is a command tick boundary.
    pub fn is_command_tick(&self) -> bool {
        self.tick % COMMAND_TICK == 0
    }

    /// Remaining army/buildings/banked value for timeout scoring.
    pub fn remaining_value(&self, player: Player) -> i32 {
        let mut value = self.ore[player.index()];
        for u in &self.units {
            if u.owner == player {
                value += unit_stats(u.utype).cost;
            }
        }
        for b in &self.buildings {
            if b.owner == player {
                value += building_stats(b.btype).cost;
            }
        }
        value
    }

    /// Check and record the win condition. Call at the end of each tick.
    pub fn check_win(&mut self) {
        if self.is_over() {
            return;
        }
        // HQ destroyed?
        let p0_dead = self.hq(Player::P0).is_none();
        let p1_dead = self.hq(Player::P1).is_none();
        if p0_dead && p1_dead {
            self.winner = None;
            self.win_reason = Some(WinReason::HqDestroyed);
            self.over = true;
            return;
        }
        if p0_dead {
            self.winner = Some(Player::P1);
            self.win_reason = Some(WinReason::HqDestroyed);
            self.over = true;
            return;
        }
        if p1_dead {
            self.winner = Some(Player::P0);
            self.win_reason = Some(WinReason::HqDestroyed);
            self.over = true;
            return;
        }
        // Timeout?
        if self.tick >= self.config.timeout_ticks {
            let v0 = self.remaining_value(Player::P0);
            let v1 = self.remaining_value(Player::P1);
            self.win_reason = Some(WinReason::Timeout);
            self.winner = match v0.cmp(&v1) {
                std::cmp::Ordering::Greater => Some(Player::P0),
                std::cmp::Ordering::Less => Some(Player::P1),
                std::cmp::Ordering::Equal => None,
            };
            self.over = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Stance, UnitType};

    fn game() -> Game {
        Game::new(Map::generate(1), GameConfig::default())
    }

    #[test]
    fn starts_with_two_hqs_and_ore() {
        let g = game();
        assert_eq!(g.buildings.len(), 2);
        assert_eq!(g.ore, [450, 450]);
        assert_eq!(g.tick, 0);
        // Each side starts with one harvester (visible mining loop).
        assert_eq!(
            g.units
                .iter()
                .filter(|u| u.utype == UnitType::Harvester)
                .count(),
            2
        );
    }

    #[test]
    fn illegal_commands_rejected() {
        // Open, deterministic map: HQs at (8,8) and (55,55).
        let mut g = Game::new(crate::map::open_test_map(1), GameConfig::default());
        // Building the HQ itself is illegal.
        let err = g
            .apply_commands(
                Player::P0,
                &[Command::PlaceBuilding {
                    player: Player::P0,
                    btype: BuildingType::Hq,
                    tile: (10, 10),
                }],
            )
            .remove(0);
        assert_eq!(err, Err(CommandError::NotABuilding));
        // Far from P0's HQ (8,8): outside the placement radius.
        let err = g
            .apply_commands(
                Player::P0,
                &[Command::PlaceBuilding {
                    player: Player::P0,
                    btype: BuildingType::Refinery,
                    tile: (60, 60),
                }],
            )
            .remove(0);
        assert_eq!(err, Err(CommandError::TooFarFromBase));
    }

    #[test]
    fn refinery_placement_charges_ore() {
        let mut g = game();
        // Place a refinery within build radius of the HQ.
        let hq = g.hq(Player::P0).unwrap().tile;
        let tile = (hq.0 + 2, hq.1);
        let res = g.apply_commands(
            Player::P0,
            &[Command::PlaceBuilding {
                player: Player::P0,
                btype: BuildingType::Refinery,
                tile,
            }],
        );
        assert_eq!(res, vec![Ok(())]);
        assert_eq!(g.ore[0], 150);
        assert_eq!(g.buildings.len(), 3);
    }

    #[test]
    fn apm_budget_drops_excess() {
        let mut g = game();
        // Exhaust the budget with many moves of a non-existent group (still
        // validated first) — use valid build spam instead via direct budget.
        for _ in 0..1000 {
            g.apm[0].try_spend();
        }
        assert!(!g.apm[0].try_spend());
    }

    #[test]
    fn train_requires_producing_building() {
        let mut g = game();
        let hq = g.hq(Player::P0).unwrap().id;
        let err = g
            .apply_commands(
                Player::P0,
                &[Command::TrainUnit {
                    player: Player::P0,
                    building: hq,
                    utype: UnitType::Infantry,
                }],
            )
            .remove(0);
        assert_eq!(err, Err(CommandError::BuildingCannotTrain));
    }

    #[test]
    fn move_requires_own_units() {
        let mut g = game();
        let err = g
            .apply_commands(
                Player::P0,
                &[Command::MoveGroup {
                    player: Player::P0,
                    units: vec![99],
                    waypoint: (20, 20),
                    stance: Stance::Aggressive,
                }],
            )
            .remove(0);
        assert_eq!(err, Err(CommandError::NotYourEntity));
    }

    #[test]
    fn power_calculation_and_low_power_production() {
        let mut g = game();
        // Initial state: HQ gives +50 power, 0 consumed
        assert_eq!(g.power(Player::P0), (50, 0));
        assert!(!g.has_low_power(Player::P0));

        let hq = g.hq(Player::P0).unwrap().tile;
        // Place a PowerPlant (+100 power)
        g.apply_commands(
            Player::P0,
            &[Command::PlaceBuilding {
                player: Player::P0,
                btype: BuildingType::PowerPlant,
                tile: (hq.0 + 1, hq.1),
            }],
        );
        assert_eq!(g.power(Player::P0), (150, 0));

        // Place a Refinery (-20 power)
        g.apply_commands(
            Player::P0,
            &[Command::PlaceBuilding {
                player: Player::P0,
                btype: BuildingType::Refinery,
                tile: (hq.0 + 2, hq.1),
            }],
        );
        assert_eq!(g.power(Player::P0), (150, 20));
        assert!(!g.has_low_power(Player::P0));
    }

    #[test]
    fn repair_building_restores_hp_and_costs_ore() {
        let mut g = game();
        let hq_id = g.hq(Player::P0).unwrap().id;

        // Full health repair rejected
        let err = g
            .apply_commands(
                Player::P0,
                &[Command::Repair {
                    player: Player::P0,
                    building: hq_id,
                }],
            )
            .remove(0);
        assert_eq!(err, Err(CommandError::BuildingFullHealth));

        // Damage the HQ
        g.building_mut(Player::P0, hq_id).unwrap().hp = 1300;
        let ore_before = g.ore[Player::P0.index()];

        // Repair HQ (+100 HP, costs 15 ore)
        let res = g
            .apply_commands(
                Player::P0,
                &[Command::Repair {
                    player: Player::P0,
                    building: hq_id,
                }],
            )
            .remove(0);
        assert_eq!(res, Ok(()));
        assert_eq!(g.building(Player::P0, hq_id).unwrap().hp, 1400);
        assert_eq!(g.ore[Player::P0.index()], ore_before - 15);
    }

    #[test]
    fn simultaneous_hq_destruction_is_a_draw() {
        let mut g = game();
        g.buildings.clear();

        g.check_win();

        assert!(g.is_over());
        assert_eq!(g.winner, None);
        assert_eq!(g.win_reason, Some(WinReason::HqDestroyed));
    }

    #[test]
    fn equal_timeout_value_is_a_draw() {
        let mut g = Game::new(
            Map::generate(1),
            GameConfig {
                timeout_ticks: 0,
                ..GameConfig::default()
            },
        );

        g.check_win();

        assert!(g.is_over());
        assert_eq!(g.winner, None);
        assert_eq!(g.win_reason, Some(WinReason::Timeout));
    }
}
