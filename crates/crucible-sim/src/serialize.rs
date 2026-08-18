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

/// Rebuild the game state at an arbitrary tick, re-applying the command log
/// exactly as it happened. Unlike [`replay_to_game`] (which advances to the
/// *last* command first), this steps only to `tick`, so it supports seeking
/// both forward and backward and is the basis for replay scrubbing.
///
/// Commands issued at the same tick are applied in their recorded order
/// before that tick is stepped, matching the live match loop.
pub fn replay_at_tick(replay: &Replay, tick: i32) -> Game {
    let mut game = Game::new(
        crate::map::Map::generate(replay.map_seed),
        replay.config.clone(),
    );
    let mut cmds = replay.commands.iter().peekable();
    while game.tick < tick && !game.is_over() {
        while let Some(c) = cmds.peek() {
            if c.tick == game.tick {
                game.apply_commands(c.player, std::slice::from_ref(&c.command));
                cmds.next();
            } else {
                break;
            }
        }
        game.step();
    }
    game
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuildingType, Command, Map, Player};

    /// Drive a short match with a deterministic scripted commander, capturing
    /// the serialized state at chosen ticks, while recording the same commands
    /// into a replay. `replay_at_tick` must reproduce each captured tick.
    fn drive_and_capture(
        seed: u64,
        timeout_ticks: i32,
        capture_ticks: &[i32],
    ) -> (Replay, Vec<Vec<u8>>) {
        let cfg = GameConfig {
            starting_ore: 100_000,
            timeout_ticks,
            ..GameConfig::default()
        };
        let mut game = Game::new(Map::generate(seed), cfg.clone());
        let mut replay = Replay::new(seed, cfg);
        let mut captures = Vec::new();
        let mut next = 0usize;
        let mut refineries = 0usize;

        // Tick 0 is the pre-command initial state (no step has run yet).
        if next < capture_ticks.len() && capture_ticks[next] == 0 {
            captures.push(snapshot_bytes(&game));
            next += 1;
        }

        while !game.is_over() {
            if game.is_command_tick() && refineries < 2 {
                let hq = game.hq(Player::P0).unwrap().tile;
                let (dx, dy) = if refineries == 0 {
                    (2i32, 0i32)
                } else {
                    (0, 2)
                };
                let cmd = Command::PlaceBuilding {
                    player: Player::P0,
                    btype: BuildingType::Refinery,
                    tile: ((hq.0 as i32 + dx) as u8, (hq.1 as i32 + dy) as u8),
                };
                replay.record(game.tick, Player::P0, cmd.clone());
                game.apply_commands(Player::P0, &[cmd]);
                refineries += 1;
            }
            // A *mid-tick* human command (tick 21 falls between command-tick
            // boundaries, which are multiples of COMMAND_TICK = 20). The live
            // server applies human commands on arrival, so replay_at_tick must
            // reproduce it byte-for-byte.
            if game.tick == 21 {
                let hq = game.hq(Player::P0).unwrap().tile;
                let cmd = Command::PlaceBuilding {
                    player: Player::P0,
                    btype: BuildingType::Refinery,
                    tile: ((hq.0 as i32 + 2) as u8, (hq.1 as i32 + 2) as u8),
                };
                replay.record(game.tick, Player::P0, cmd.clone());
                game.apply_commands(Player::P0, &[cmd]);
            }
            game.step();
            if next < capture_ticks.len() && game.tick == capture_ticks[next] {
                captures.push(snapshot_bytes(&game));
                next += 1;
            }
        }
        replay.result = Some(ReplayResult {
            winner: game.winner,
            reason: game.win_reason,
            duration_ticks: game.tick,
        });
        (replay, captures)
    }

    #[test]
    fn replay_at_tick_matches_direct_stepping() {
        let capture_ticks = [0i32, 1, 19, 20, 21, 40, 41, 100, 200];
        let (replay, captures) = drive_and_capture(2024, 300, &capture_ticks);
        assert_eq!(captures.len(), capture_ticks.len());
        for (tick, expected) in capture_ticks.iter().zip(&captures) {
            assert_eq!(
                &snapshot_bytes(&replay_at_tick(&replay, *tick)),
                expected,
                "replay_at_tick diverged at tick {tick}"
            );
        }
    }

    #[test]
    fn replay_at_tick_past_end_equals_finish() {
        let (replay, _) = drive_and_capture(7, 300, &[]);
        let finished = finish_replay(&replay);
        let end = finished.tick;
        assert_eq!(
            snapshot_bytes(&replay_at_tick(&replay, end)),
            snapshot_bytes(&replay_at_tick(&replay, end + 999)),
            "seeking past the end must stay at the final state"
        );
        assert_eq!(
            snapshot_bytes(&replay_at_tick(&replay, end)),
            snapshot_bytes(&finished),
            "replay_at_tick at the end must equal finish_replay"
        );
    }
}
