//! The complete player action space and its single validator.
//!
//! Humans, AI commanders, ghosts, and tests all issue commands through
//! [`Game::validate_command`]. Illegal commands are rejected with a reason;
//! there is no separate "AI path" that bypasses validation.

use serde::{Deserialize, Serialize};

use crate::entity::{
    building_produces, BuildingType, EntityId, Player, Stance, UnitType, Upgrade,
    PLACE_RADIUS_TILES,
};
use crate::game::Game;
use crate::map::tile_index;

/// A player command. Serialized as a tagged enum for the wire/replay format.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum Command {
    PlaceBuilding {
        player: Player,
        btype: BuildingType,
        tile: (u8, u8),
    },
    TrainUnit {
        player: Player,
        building: EntityId,
        utype: UnitType,
    },
    MoveGroup {
        player: Player,
        units: Vec<EntityId>,
        waypoint: (u8, u8),
        stance: Stance,
    },
    SetRally {
        player: Player,
        building: EntityId,
        waypoint: (u8, u8),
    },
    ChooseUpgrade {
        player: Player,
        lab: EntityId,
        upgrade: Upgrade,
    },
    Sell {
        player: Player,
        building: EntityId,
    },
    Repair {
        player: Player,
        building: EntityId,
    },
}

impl Command {
    pub fn player(&self) -> Player {
        match self {
            Command::PlaceBuilding { player, .. }
            | Command::TrainUnit { player, .. }
            | Command::MoveGroup { player, .. }
            | Command::SetRally { player, .. }
            | Command::ChooseUpgrade { player, .. }
            | Command::Sell { player, .. }
            | Command::Repair { player, .. } => *player,
        }
    }

    /// Clone this command with its `player` field replaced (used by ghosts to
    /// replay a recorded command stream on a fresh match side).
    pub fn with_player(&self, player: Player) -> Command {
        let mut c = self.clone();
        match &mut c {
            Command::PlaceBuilding { player: p, .. }
            | Command::TrainUnit { player: p, .. }
            | Command::MoveGroup { player: p, .. }
            | Command::SetRally { player: p, .. }
            | Command::ChooseUpgrade { player: p, .. }
            | Command::Sell { player: p, .. }
            | Command::Repair { player: p, .. } => *p = player,
        }
        c
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum CommandError {
    NotYourEntity,
    EntityDead,
    NotABuilding,
    NotAUnit,
    InvalidTile,
    TileBlocked,
    TileHasOre,
    TooFarFromBase,
    NotEnoughOre,
    BuildingCannotTrain,
    RequiresTechLab,
    RequiresFactory,
    UpgradeAlreadyChosen,
    CantSellHq,
    BuildingFullHealth,
    EmptyGroup,
    QueueFull,
    /// Command dropped by the APM budget (rate limit).
    RateLimited,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Game {
    /// Validate a command against current state. Pure — mutates nothing.
    pub fn validate_command(&self, cmd: &Command) -> Result<(), CommandError> {
        use CommandError::*;
        match cmd {
            Command::PlaceBuilding {
                player,
                btype,
                tile,
            } => self.validate_place(*player, *btype, *tile),
            Command::TrainUnit {
                player,
                building,
                utype,
            } => self.validate_train(*player, *building, *utype),
            Command::MoveGroup {
                player,
                units,
                waypoint,
                ..
            } => self.validate_move(*player, units, *waypoint),
            Command::SetRally {
                player,
                building,
                waypoint,
            } => {
                let b = self.building(*player, *building).ok_or(NotYourEntity)?;
                if !b.is_alive() {
                    return Err(EntityDead);
                }
                if !building_produces(b.btype).is_empty() {
                    self.validate_tile(*waypoint)?;
                    Ok(())
                } else {
                    Err(NotABuilding)
                }
            }
            Command::ChooseUpgrade {
                player,
                lab,
                upgrade,
            } => {
                if *upgrade == Upgrade::None {
                    return Err(UpgradeAlreadyChosen);
                }
                let b = self.building(*player, *lab).ok_or(NotYourEntity)?;
                if !b.is_alive() {
                    return Err(EntityDead);
                }
                if b.btype != BuildingType::TechLab {
                    return Err(NotABuilding);
                }
                if self.upgrades[player.index()] != Upgrade::None {
                    return Err(UpgradeAlreadyChosen);
                }
                Ok(())
            }
            Command::Sell { player, building } => {
                let b = self.building(*player, *building).ok_or(NotYourEntity)?;
                if !b.is_alive() {
                    return Err(EntityDead);
                }
                if b.btype == BuildingType::Hq {
                    return Err(CantSellHq);
                }
                Ok(())
            }
            Command::Repair { player, building } => {
                let b = self.building(*player, *building).ok_or(NotYourEntity)?;
                if !b.is_alive() {
                    return Err(EntityDead);
                }
                if b.hp >= b.max_hp {
                    return Err(BuildingFullHealth);
                }
                if self.ore[player.index()] < 15 {
                    return Err(NotEnoughOre);
                }
                Ok(())
            }
        }
    }

    fn validate_place(
        &self,
        player: Player,
        btype: BuildingType,
        tile: (u8, u8),
    ) -> Result<(), CommandError> {
        use CommandError::*;
        if btype == BuildingType::Hq {
            return Err(NotABuilding);
        }
        if (btype == BuildingType::TechLab || btype == BuildingType::Airfield)
            && self.count_buildings(player, BuildingType::Factory) == 0
        {
            return Err(RequiresFactory);
        }
        self.validate_tile(tile)?;
        if self.building_at(tile).is_some() {
            return Err(TileBlocked);
        }
        if self.map.ore_at(tile.0, tile.1) > 0 {
            return Err(TileHasOre);
        }
        let cost = crate::entity::building_stats(btype).cost;
        if self.ore[player.index()] < cost {
            return Err(NotEnoughOre);
        }
        if !self.near_own_building(player, tile) {
            return Err(TooFarFromBase);
        }
        Ok(())
    }

    fn validate_train(
        &self,
        player: Player,
        building: EntityId,
        utype: UnitType,
    ) -> Result<(), CommandError> {
        use CommandError::*;
        let b = self.building(player, building).ok_or(NotYourEntity)?;
        if !b.is_alive() {
            return Err(EntityDead);
        }
        if !building_produces(b.btype).contains(&utype) {
            return Err(BuildingCannotTrain);
        }
        if utype == UnitType::Artillery && self.count_buildings(player, BuildingType::TechLab) == 0
        {
            return Err(RequiresTechLab);
        }
        let cost = crate::entity::unit_stats(utype).cost;
        if self.ore[player.index()] < cost {
            return Err(NotEnoughOre);
        }
        if b.queue.len() >= self.config.max_queue {
            return Err(QueueFull);
        }
        Ok(())
    }

    fn validate_move(
        &self,
        player: Player,
        units: &[EntityId],
        waypoint: (u8, u8),
    ) -> Result<(), CommandError> {
        use CommandError::*;
        if units.is_empty() {
            return Err(EmptyGroup);
        }
        for id in units {
            if self.unit(player, *id).is_none() {
                return Err(NotYourEntity);
            }
        }
        self.validate_tile(waypoint)?;
        Ok(())
    }

    fn validate_tile(&self, tile: (u8, u8)) -> Result<(), CommandError> {
        if tile.0 as usize >= crate::map::MAP_SIZE || tile.1 as usize >= crate::map::MAP_SIZE {
            return Err(CommandError::InvalidTile);
        }
        if !self.map.is_passable(tile.0, tile.1) {
            return Err(CommandError::TileBlocked);
        }
        Ok(())
    }

    /// The target tile must be within [`PLACE_RADIUS_TILES`] of at least one own
    /// building (any building, including ones still under construction). This is
    /// what keeps a base in one connected clump instead of scattered structures.
    fn near_own_building(&self, player: Player, tile: (u8, u8)) -> bool {
        let cx = crate::fixed::tile_center(tile.0);
        let cy = crate::fixed::tile_center(tile.1);
        let lim = (PLACE_RADIUS_TILES as i64 * crate::fixed::FIX_SCALE as i64)
            * (PLACE_RADIUS_TILES as i64 * crate::fixed::FIX_SCALE as i64);
        self.buildings
            .iter()
            .filter(|b| b.owner == player)
            .any(|b| {
                let px = crate::fixed::tile_center(b.tile.0);
                let py = crate::fixed::tile_center(b.tile.1);
                crate::fixed::dist2(cx, cy, px, py) <= lim
            })
    }

    fn count_buildings(&self, player: Player, btype: BuildingType) -> usize {
        self.buildings
            .iter()
            .filter(|b| b.owner == player && b.btype == btype)
            .count()
    }

    #[allow(dead_code)]
    fn tile_is_empty(&self, tile: (u8, u8)) -> bool {
        self.building_at(tile).is_none()
    }
}

/// The number of commands a player has issued since the last APM check (for
/// debugging/tuning counters).
#[allow(dead_code)]
pub fn command_tile(cmd: &Command) -> (u8, u8) {
    match cmd {
        Command::PlaceBuilding { tile, .. } | Command::SetRally { waypoint: tile, .. } => *tile,
        Command::MoveGroup { waypoint, .. } => *waypoint,
        _ => (0, 0),
    }
}

#[allow(dead_code)]
fn _tile_index_reexport(x: u8, y: u8) -> usize {
    tile_index(x, y)
}
