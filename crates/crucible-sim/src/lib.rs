//! # crucible-sim
//!
//! The pure, deterministic simulation core for CRUCIBLE. No IO, no threads,
//! no OS calls, no wall clock. Compiles identically for native and wasm.
//!
//! The determinism contract:
//! - All randomness flows through the injected seeded [`Rng`](rng::Rng).
//! - Fixed timestep (10 ticks/second); one [`Game::step`] call per tick.
//! - Entities are iterated in ascending id order everywhere.
//! - Integer math only (see [`fixed`]); no platform-variable float functions
//!   in game-state math.
//! - [`Game`] is fully serializable via serde at any tick.

pub mod combat;
pub mod economy;
pub mod entity;
pub mod fixed;
pub mod fog;
pub mod game;
pub mod map;
pub mod movement;
pub mod orders;
pub mod rng;
pub mod serialize;
pub mod tick;

pub use entity::{
    building_stats, unit_stats, Building, BuildingType, EntityId, Player, Stance, Unit, UnitOrder,
    UnitType, Upgrade,
};
pub use fixed::{Fix, Pos, COMMAND_TICK, MATCH_TIMEOUT_TICKS, TICKS_PER_SEC};
pub use game::{EventKind, Game, GameConfig, GameEvent, WinReason};
pub use map::{open_test_map, Map};
pub use orders::{Command, CommandError};
pub use rng::Rng;
pub use serialize::{Replay, ReplayResult, TimedCommand, FORMAT_VERSION};

/// Library version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
