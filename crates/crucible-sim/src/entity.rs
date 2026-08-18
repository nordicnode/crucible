//! Entities (units & buildings) and the v1 balance tables.
//!
//! All combat/economy stats are plain integers so they behave identically on
//! every target. Entity ids are assigned in ascending creation order; every
//! sim phase iterates entities in ascending id order (part of the determinism
//! contract).

use serde::{Deserialize, Serialize};

use crate::fixed::{Fix, Pos, FIX_SCALE, TICKS_PER_SEC};

/// Two players. Serialized as 0 / 1.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Debug)]
#[repr(u8)]
pub enum Player {
    P0 = 0,
    P1 = 1,
}

impl Player {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn enemy(self) -> Player {
        match self {
            Player::P0 => Player::P1,
            Player::P1 => Player::P0,
        }
    }

    pub const ALL: [Player; 2] = [Player::P0, Player::P1];
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Debug)]
pub enum BuildingType {
    Hq,
    Refinery,
    Barracks,
    Factory,
    TechLab,
    Turret,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Debug)]
pub enum UnitType {
    Harvester,
    Infantry,
    Tank,
    Artillery,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Debug)]
pub enum Stance {
    /// Engage enemies in aggro range while moving; standard attack-move.
    Aggressive,
    /// Like aggressive, but retreat when below 20% HP.
    Cautious,
    /// Move to waypoint then hold position; attack only in range.
    Hold,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Debug)]
pub enum Upgrade {
    None,
    Damage,
    Hp,
}

/// Unique entity id within a match.
pub type EntityId = u32;

/// Static, per-type balance data. Distances/radii are in fix units,
/// durations in ticks, ore in integer units.
#[derive(Clone, Copy, Debug)]
pub struct UnitStats {
    pub cost: i32,
    pub hp: i32,
    pub damage: i32,
    /// Attack range in fix units (0 = cannot attack).
    pub range: Fix,
    /// Minimum attack range in fix units (artillery cannot fire inside this).
    pub min_range: Fix,
    /// Movement per tick in fix units.
    pub speed: Fix,
    /// Attack cooldown in ticks.
    pub cooldown: i32,
    /// Vision radius in fix units.
    pub vision: Fix,
    /// Splash radius in fix units (0 = single target).
    pub splash: Fix,
    /// Production time in ticks.
    pub build_time: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct BuildingStats {
    pub cost: i32,
    pub hp: i32,
    pub vision: Fix,
    /// Passive ore trickle per tick (refinery only).
    pub trickle: i32,
    /// Attack damage (turrets only).
    pub damage: i32,
    /// Attack range in fix units (turrets only).
    pub range: Fix,
    /// Attack cooldown in ticks (turrets only).
    pub cooldown: i32,
}

const fn tiles(t: i32) -> Fix {
    t * FIX_SCALE
}

pub const fn unit_stats(ut: UnitType) -> UnitStats {
    use UnitType::*;
    match ut {
        Harvester => UnitStats {
            cost: 100,
            hp: 80,
            damage: 0,
            range: 0,
            min_range: 0,
            speed: 23, // 0.9 tiles/s
            cooldown: 0,
            vision: tiles(4),
            splash: 0,
            build_time: TICKS_PER_SEC * 20,
        },
        Infantry => UnitStats {
            cost: 50,
            hp: 44,
            damage: 6,
            range: tiles(1) + FIX_SCALE / 2, // 1.5 tiles
            min_range: 0,
            speed: 34, // ~1.35 tiles/s (closes on artillery)
            cooldown: TICKS_PER_SEC,
            vision: tiles(4),
            splash: 0,
            build_time: TICKS_PER_SEC * 8,
        },
        Tank => UnitStats {
            cost: 150,
            hp: 120,
            damage: 20,
            range: tiles(3),
            min_range: 0,
            speed: 20, // 0.8 tiles/s
            cooldown: TICKS_PER_SEC * 12 / 10,
            vision: tiles(5),
            splash: 0,
            build_time: TICKS_PER_SEC * 16,
        },
        // Balance-tuned for positional combat: `min_range`/`cooldown` keep
        // artillery a counter to tanks (outranges them) while infantry closes
        // inside its min range. Tanks carry no splash; they counter infantry
        // by range + fire rate. See `crucible-evo/tests/fixtures/balance_baseline.json`.
        Artillery => UnitStats {
            cost: 200,
            hp: 60,
            damage: 32,
            range: tiles(6),
            min_range: tiles(1) / 2, // 0.5 tile min range
            speed: 12,               // 0.5 tiles/s
            cooldown: TICKS_PER_SEC * 18 / 10,
            vision: tiles(6),
            splash: 0,
            build_time: TICKS_PER_SEC * 20,
        },
    }
}

pub const fn building_stats(bt: BuildingType) -> BuildingStats {
    use BuildingType::*;
    match bt {
        Hq => BuildingStats {
            cost: 0,
            hp: 1500,
            vision: tiles(5),
            trickle: 0,
            damage: 0,
            range: 0,
            cooldown: 0,
        },
        Refinery => BuildingStats {
            cost: 300,
            hp: 400,
            vision: tiles(3),
            trickle: 1,
            damage: 0,
            range: 0,
            cooldown: 0,
        },
        Barracks => BuildingStats {
            cost: 150,
            hp: 300,
            vision: tiles(3),
            trickle: 0,
            damage: 0,
            range: 0,
            cooldown: 0,
        },
        Factory => BuildingStats {
            cost: 250,
            hp: 400,
            vision: tiles(3),
            trickle: 0,
            damage: 0,
            range: 0,
            cooldown: 0,
        },
        TechLab => BuildingStats {
            cost: 200,
            hp: 250,
            vision: tiles(3),
            trickle: 0,
            damage: 0,
            range: 0,
            cooldown: 0,
        },
        Turret => BuildingStats {
            cost: 100,
            hp: 150,
            vision: tiles(4),
            trickle: 0,
            damage: 12,
            range: tiles(3) + FIX_SCALE / 2,
            cooldown: TICKS_PER_SEC * 8 / 10,
        },
    }
}

/// How much ore a harvester carries per trip.
pub const HARVESTER_CAPACITY: i32 = 50;
/// Ticks a harvester spends mining to fill its hold.
pub const HARVEST_TICKS: i32 = TICKS_PER_SEC * 5;
/// Ore extracted from a field per harvest tick.
pub const HARVEST_RATE_PER_TICK: i32 = HARVESTER_CAPACITY / HARVEST_TICKS;

/// Which units a building can train.
pub fn building_produces(bt: BuildingType) -> &'static [UnitType] {
    use BuildingType::*;
    use UnitType::*;
    match bt {
        Barracks => &[Infantry],
        Factory => &[Harvester, Tank],
        _ => &[],
    }
}

/// Where a building may be placed: within this many tiles (center-to-center)
/// of the *nearest own building*, so bases grow in connected clumps instead
/// of floating structures. Balance-tuned with the baseline fixture.
pub const PLACE_RADIUS_TILES: i32 = 5;

/// Aggro radius: units engage enemies they can see (vision == aggro for v1).
pub const RETREAT_HP_FRACTION_NUM: i32 = 1;
pub const RETREAT_HP_FRACTION_DEN: i32 = 5; // 20%

/// A building in the world.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Building {
    pub id: EntityId,
    pub owner: Player,
    pub btype: BuildingType,
    pub tile: (u8, u8),
    pub hp: i32,
    pub max_hp: i32,
    /// Pending production queue (unit types).
    pub queue: Vec<UnitType>,
    /// Progress toward the current queued unit, in ticks.
    pub progress: i32,
    /// Rally point for produced units.
    pub rally: Option<(u8, u8)>,
    /// Attack cooldown (turret).
    pub cooldown: i32,
}

impl Building {
    pub fn pos(&self) -> Pos {
        Pos::from_tile(self.tile.0, self.tile.1)
    }

    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }
}

/// What a unit is currently doing.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum UnitOrder {
    Idle,
    /// Move to a waypoint with a stance.
    Move {
        waypoint: Pos,
        stance: Stance,
    },
    /// Explicitly attack a specific entity.
    Attack {
        target: EntityId,
    },
    /// Harvester loop (find field, mine, return to refinery).
    Harvest,
    /// Flee toward a position.
    Flee {
        dest: Pos,
    },
}

/// A unit in the world.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Unit {
    pub id: EntityId,
    pub owner: Player,
    pub utype: UnitType,
    pub pos: Pos,
    pub hp: i32,
    pub max_hp: i32,
    pub stance: Stance,
    pub order: UnitOrder,
    /// Ore currently carried (harvesters).
    pub carrying: i32,
    /// Attack cooldown remaining (ticks).
    pub cooldown: i32,
    /// Mining progress accumulator (harvesters), in harvest ticks.
    pub mining: i32,
    /// Remaining path (tile waypoints) for long-range navigation.
    pub path: Vec<(u8, u8)>,
    /// Current attack target (re-evaluated per tick).
    pub target: Option<EntityId>,
    /// Whether currently fleeing (cautious stance at low HP).
    pub fleeing: bool,
    /// Ore tile a harvester is currently mining.
    pub harvest_tile: Option<(u8, u8)>,
    /// Refinery a harvester is returning to.
    pub refinery: Option<EntityId>,
}

impl Unit {
    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }
}

/// Aggregate entity container (units + buildings), id-assignment order stable.
#[derive(Default)]
pub struct EntityAllocator {
    next_id: EntityId,
}

impl EntityAllocator {
    pub fn new() -> Self {
        EntityAllocator { next_id: 1 }
    }

    pub fn alloc(&mut self) -> EntityId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn peek_next(&self) -> EntityId {
        self.next_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artillery_has_min_range() {
        assert!(unit_stats(UnitType::Artillery).min_range > 0);
    }

    #[test]
    fn tank_outranges_infantry() {
        // Tank beats infantry by range + rate, not splash (see the M8
        // balance note: splash was removed during positional-combat tuning).
        assert!(unit_stats(UnitType::Tank).range > unit_stats(UnitType::Infantry).range);
    }

    #[test]
    fn only_barracks_and_factory_produce() {
        assert_eq!(
            building_produces(BuildingType::Barracks),
            &[UnitType::Infantry]
        );
        assert_eq!(
            building_produces(BuildingType::Factory),
            &[UnitType::Harvester, UnitType::Tank]
        );
        assert!(building_produces(BuildingType::Hq).is_empty());
    }
}
