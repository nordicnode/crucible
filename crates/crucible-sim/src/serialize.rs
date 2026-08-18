//! Snapshot and replay serialization.
//!
//! A snapshot is just the serialized [`Game`]; a replay is the *input log*
//! (map seed + ordered commands + result) so it is a few KB, not a state dump.
//! Formats are versioned from day one.

use serde::{Deserialize, Serialize};

use crate::entity::Player;
use crate::game::{Game, GameConfig, WinReason};
use crate::orders::Command;

pub const FORMAT_VERSION: u32 = 1;

/// A command stamped with the tick at which it was issued.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct TimedCommand {
    pub tick: i32,
    pub player: Player,
    pub command: Command,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ReplayResult {
    pub winner: Option<Player>,
    pub reason: Option<WinReason>,
    pub duration_ticks: i32,
}

/// The replay (input log) format. Versioned so old replays stay re-runnable.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Replay {
    pub version: u32,
    pub map_seed: u64,
    pub config: GameConfig,
    pub commands: Vec<TimedCommand>,
    pub result: Option<ReplayResult>,
}

impl Replay {
    pub fn new(map_seed: u64, config: GameConfig) -> Self {
        Replay {
            version: FORMAT_VERSION,
            map_seed,
            config,
            commands: Vec::new(),
            result: None,
        }
    }

    pub fn record(&mut self, tick: i32, player: Player, command: Command) {
        self.commands.push(TimedCommand {
            tick,
            player,
            command,
        });
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("replay serialization is infallible")
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Serialize a full game state to a canonical JSON string.
///
/// Field order is stable (struct definition order), so this is byte-identical
/// for identical states on every target. Used by golden determinism tests.
pub fn snapshot_json(game: &Game) -> String {
    serde_json::to_string(game).expect("game serialization is infallible")
}

/// Serialize a full game state to canonical JSON bytes (for hashing).
pub fn snapshot_bytes(game: &Game) -> Vec<u8> {
    serde_json::to_vec(game).expect("game serialization is infallible")
}

/// Rebuild a fresh game from a replay's seed and re-apply its command log to
/// reproduce the match exactly.
pub fn replay_to_game(replay: &Replay) -> Game {
    let mut game = Game::new(
        crate::map::Map::generate(replay.map_seed),
        replay.config.clone(),
    );
    // Commands are already tick-ordered by construction; apply in order,
    // stepping the sim forward as needed to reach each command's tick.
    let mut applied = 0usize;
    while applied < replay.commands.len() {
        let cmd = &replay.commands[applied];
        while game.tick < cmd.tick && !game.is_over() {
            game.step();
        }
        if !game.is_over() {
            game.apply_commands(cmd.player, std::slice::from_ref(&cmd.command));
        }
        applied += 1;
    }
    game
}

/// Run a game to completion from a replay (continues past the last command).
pub fn finish_replay(replay: &Replay) -> Game {
    let mut game = replay_to_game(replay);
    let mut guard = 0;
    while !game.is_over() && guard < crate::fixed::MATCH_TIMEOUT_TICKS + 1000 {
        game.step();
        guard += 1;
    }
    game
}
