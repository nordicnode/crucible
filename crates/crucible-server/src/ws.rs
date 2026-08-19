//! WebSocket live-match protocol. Server-authoritative: the sim runs here at
//! the fixed timestep, human commands are validated identically to the bot's,
//! and the client only receives the human player's fogged view.

use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crucible_ai::{easy, hard, medium, Bot, GenomeBot};
use crucible_sim::{
    entity::BuildingType, fixed::FIX_SCALE, Command, Game, GameConfig, Map, Player, Replay,
    ReplayResult, UnitType,
};

use crate::store::Store;

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ClientMsg {
    JoinMatch { opponent: String },
    Commands { cmds: Vec<Command> },
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ServerMsg {
    MatchStart(MatchStartMsg),
    StateDiff(StateDiffMsg),
    MatchEnd(MatchEndMsg),
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct MatchStartMsg {
    map_seed: u64,
    player: u8,
    passable: Vec<bool>,
    hq: [(u8, u8); 2],
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct StateDiffMsg {
    tick: i32,
    ore: i32,
    power_produced: i32,
    power_consumed: i32,
    entities: Vec<DiffEntity>,
    ore_tiles: Vec<OreTile>,
    visible: Vec<u16>,
    events: Vec<DiffEvent>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct MatchEndMsg {
    winner: Option<u8>,
    reason: Option<crucible_sim::WinReason>,
    duration_ticks: i32,
    replay_id: Option<i64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct DiffEntity {
    id: u32,
    kind: String,
    owner: u8,
    x: f32,
    y: f32,
    hp: i32,
    max_hp: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stale: Option<i32>,
    /// Own-building production queue (unit kind names, oldest first).
    #[serde(skip_serializing_if = "Option::is_none")]
    queue: Option<Vec<String>>,
    /// Progress of the current queue head, in ticks.
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<i32>,
    /// Build time of the current queue head, in ticks.
    #[serde(skip_serializing_if = "Option::is_none")]
    build_time: Option<i32>,
    /// Rally point for produced units (tile coords [x, y]).
    #[serde(skip_serializing_if = "Option::is_none")]
    rally: Option<(u8, u8)>,
}

#[derive(Serialize, Clone, Debug)]
struct OreTile {
    x: u8,
    y: u8,
    amount: i32,
}

#[derive(Serialize, Clone, Debug)]
struct DiffEvent {
    tick: i32,
    kind: String,
    /// Deposit amount for `ore_deposited` events (null otherwise) — lets the
    /// client show real harvest income, since refineries give no passive
    /// income.
    #[serde(skip_serializing_if = "Option::is_none")]
    amount: Option<i32>,
    /// Index of the player associated with this event (0 = P0, 1 = P1).
    #[serde(skip_serializing_if = "Option::is_none")]
    player: Option<u8>,
}

pub async fn handler(
    ws: WebSocketUpgrade,
    State(state): State<crate::AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle(socket, state))
}

async fn handle(socket: WebSocket, state: crate::AppState) {
    if let Err(e) = run(socket, state).await {
        tracing::warn!("ws session ended with error: {e}");
    }
}

async fn run(socket: WebSocket, state: crate::AppState) -> Result<(), Box<dyn std::error::Error>> {
    let (mut sender, mut receiver) = socket.split();

    // Wait for JoinMatch.
    let opponent = loop {
        match receiver.next().await {
            Some(Ok(Message::Text(t))) => {
                if let Ok(ClientMsg::JoinMatch { opponent }) = serde_json::from_str(&t) {
                    break opponent;
                }
            }
            Some(Ok(Message::Close(_))) | None => return Ok(()),
            _ => continue,
        }
    };

    let mut bot: Box<dyn Bot> = resolve_opponent(state.store.as_ref(), &opponent);

    // Seed from the wall clock (server is the one place this is allowed); the
    // seed is recorded in the replay so the match stays reproducible.
    let seed = seed_now();
    let config = timeout_override(GameConfig::default());
    let mut game = Game::new(Map::generate(seed), config.clone());
    let mut replay = Replay::new(seed, config);

    let passable = game.map.passable.clone();
    let hq = [game.map.hq_tiles[0], game.map.hq_tiles[1]];

    sender
        .send(Message::Text(
            serde_json::to_string(&ServerMsg::MatchStart(MatchStartMsg {
                map_seed: seed,
                player: Player::P0.index() as u8,
                passable,
                hq,
            }))?
            .into(),
        ))
        .await?;

    // Incoming commands are buffered on a channel by a reader task.
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<Command>>();
    tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(t) = msg {
                match serde_json::from_str::<ClientMsg>(&t) {
                    Ok(ClientMsg::Commands { cmds }) => {
                        let _ = tx.send(cmds);
                    }
                    Ok(ClientMsg::JoinMatch { .. }) => {}
                    // Never drop a malformed command silently: a wire-format
                    // drift (e.g. player as index vs "P0") otherwise looks
                    // like the game ignoring the player.
                    Err(e) => tracing::warn!("dropping unparseable client message: {e}: {t}"),
                }
            }
        }
    });

    let mut pending: Vec<Command> = Vec::new();
    let mut last_event_tick = 0i32;
    let mut tick_interval = tokio::time::interval(Duration::from_millis(100));

    loop {
        // Drain buffered human commands.
        while let Ok(cmds) = rx.try_recv() {
            pending.extend(cmds);
        }

        // The AI opponent (P1) deliberates on the command-tick cadence
        // (COMMAND_TICK = 20 sim ticks, i.e. every 2 s).
        if game.is_command_tick() {
            let bot_cmds = bot.decide(&game, Player::P1);
            for c in &bot_cmds {
                replay.record(game.tick, Player::P1, c.clone());
            }
            game.apply_commands(Player::P1, &bot_cmds);
        }

        // Human (P0) commands apply the tick they arrive — no waiting for the
        // next command-tick boundary. Replays record the exact tick, and the
        // replay re-runs (spectate shim, ghosts) apply commands at arbitrary
        // ticks, so this stays byte-deterministic.
        let human_cmds = std::mem::take(&mut pending);
        for c in &human_cmds {
            replay.record(game.tick, Player::P0, c.clone());
        }
        game.apply_commands(Player::P0, &human_cmds);

        game.step();

        let diff = build_diff(&game, &mut last_event_tick);
        sender
            .send(Message::Text(serde_json::to_string(&diff)?.into()))
            .await?;

        if game.is_over() {
            let result = ReplayResult {
                winner: game.winner,
                reason: game.win_reason,
                duration_ticks: game.tick,
            };
            replay.result = Some(result);
            let replay_id = state
                .store
                .save_match(
                    seed,
                    "human",
                    &format!("bot:{opponent}"),
                    &format!("{:?}", game.winner),
                    game.tick,
                    &replay.to_json(),
                )
                .ok();
            sender
                .send(Message::Text(
                    serde_json::to_string(&ServerMsg::MatchEnd(MatchEndMsg {
                        winner: game.winner.map(|p| p.index() as u8),
                        reason: game.win_reason,
                        duration_ticks: game.tick,
                        replay_id,
                    }))?
                    .into(),
                ))
                .await?;
            break;
        }

        tick_interval.tick().await;
    }

    Ok(())
}

fn build_diff(game: &Game, last_event_tick: &mut i32) -> ServerMsg {
    let view = game.fog_view(Player::P0);
    let mut entities = Vec::new();

    for u in &game.units {
        if u.owner == Player::P0 {
            entities.push(DiffEntity {
                id: u.id,
                kind: unit_kind(u.utype),
                owner: 0,
                x: u.pos.x as f32 / FIX_SCALE as f32,
                y: u.pos.y as f32 / FIX_SCALE as f32,
                hp: u.hp,
                max_hp: u.max_hp,
                stale: None,
                queue: None,
                progress: None,
                build_time: None,
                rally: None,
            });
        }
    }
    for b in &game.buildings {
        if b.owner == Player::P0 {
            let (queue, progress, build_time) = if !b.queue.is_empty() {
                let head = b.queue[0];
                (
                    Some(b.queue.iter().map(|u| format!("{u:?}")).collect::<Vec<_>>()),
                    Some(b.progress),
                    Some(crucible_sim::unit_stats(head).build_time),
                )
            } else {
                (None, None, None)
            };
            entities.push(DiffEntity {
                id: b.id,
                kind: building_kind(b.btype),
                owner: 0,
                x: b.tile.0 as f32 + 0.5,
                y: b.tile.1 as f32 + 0.5,
                hp: b.hp,
                max_hp: b.max_hp,
                stale: None,
                queue,
                progress,
                build_time,
                rally: b.rally,
            });
        }
    }
    // Enemy: only what the fog view exposes (last-seen + currently visible).
    for m in &view.units {
        entities.push(DiffEntity {
            id: m.id,
            kind: unit_kind(m.utype),
            owner: 1,
            x: m.pos.x as f32 / FIX_SCALE as f32,
            y: m.pos.y as f32 / FIX_SCALE as f32,
            hp: 0,
            max_hp: 0,
            stale: Some(m.last_seen),
            queue: None,
            progress: None,
            build_time: None,
            rally: None,
        });
    }
    for m in &view.buildings {
        entities.push(DiffEntity {
            id: m.id,
            kind: building_kind(m.btype),
            owner: 1,
            x: m.tile.0 as f32 + 0.5,
            y: m.tile.1 as f32 + 0.5,
            hp: 0,
            max_hp: 0,
            stale: Some(m.last_seen),
            queue: None,
            progress: None,
            build_time: None,
            rally: None,
        });
    }

    let mut ore_tiles = Vec::new();
    for idx in 0..(64 * 64) {
        if view.known_ore[idx] && game.map.ore[idx] > 0 {
            ore_tiles.push(OreTile {
                x: (idx % 64) as u8,
                y: (idx / 64) as u8,
                amount: game.map.ore[idx],
            });
        }
    }

    let visible: Vec<u16> = view
        .visible
        .iter()
        .enumerate()
        .filter(|(_, v)| **v)
        .map(|(i, _)| i as u16)
        .collect();

    let events: Vec<DiffEvent> = game
        .events
        .iter()
        .filter(|e| e.tick > *last_event_tick)
        .map(|e| {
            let player = match &e.kind {
                crucible_sim::EventKind::BuildingPlaced { player, .. }
                | crucible_sim::EventKind::UnitTrained { player, .. }
                | crucible_sim::EventKind::OreDeposited { player, .. }
                | crucible_sim::EventKind::Sold { player, .. }
                | crucible_sim::EventKind::UpgradeChosen { player, .. } => {
                    Some(player.index() as u8)
                }
                crucible_sim::EventKind::UnitDied { owner, .. }
                | crucible_sim::EventKind::BuildingDestroyed { owner, .. } => {
                    Some(owner.index() as u8)
                }
            };
            DiffEvent {
                tick: e.tick,
                kind: event_kind(&e.kind),
                amount: match &e.kind {
                    crucible_sim::EventKind::OreDeposited { amount, .. } => Some(*amount),
                    crucible_sim::EventKind::Sold { refund, .. } => Some(*refund),
                    _ => None,
                },
                player,
            }
        })
        .collect();
    *last_event_tick = game.tick;
    let (power_produced, power_consumed) = game.power(crucible_sim::Player::P0);

    ServerMsg::StateDiff(StateDiffMsg {
        tick: game.tick,
        ore: game.ore[0],
        power_produced,
        power_consumed,
        entities,
        ore_tiles,
        visible,
        events,
    })
}

fn unit_kind(u: UnitType) -> String {
    // Serde variant name, matching both the snapshot format and the client's
    // renderer/selection kind strings ("Infantry", "Tank", …).
    format!("{u:?}")
}

fn building_kind(b: BuildingType) -> String {
    format!("{b:?}")
}

fn event_kind(e: &crucible_sim::EventKind) -> String {
    match e {
        crucible_sim::EventKind::UnitTrained { utype, .. } => {
            format!("trained:{utype:?}").to_lowercase()
        }
        crucible_sim::EventKind::UnitDied { .. } => "unit_died".into(),
        crucible_sim::EventKind::BuildingDestroyed { .. } => "building_destroyed".into(),
        crucible_sim::EventKind::OreDeposited { .. } => "ore_deposited".into(),
        crucible_sim::EventKind::BuildingPlaced { btype, .. } => {
            format!("built:{btype:?}").to_lowercase()
        }
        crucible_sim::EventKind::Sold { .. } => "sold".into(),
        crucible_sim::EventKind::UpgradeChosen { upgrade, .. } => {
            format!("upgrade:{upgrade:?}").to_lowercase()
        }
    }
}
fn seed_now() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    now ^ COUNTER
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_mul(0x9E37_79B9)
}

/// Resolve a lobby opponent string to a concrete bot. Scripted bots (`easy`,
/// `medium`, `hard`) are always available; `champion` plays the reigning
/// champion and `museum:{genome_id}` plays any stored genome. Falls back to the
/// hard bot when the requested genome is missing (e.g. a fresh DB with no
/// crowned champion yet).
fn resolve_opponent(store: &Store, opponent: &str) -> Box<dyn Bot> {
    match opponent {
        "easy" => return Box::new(easy()),
        "medium" => return Box::new(medium()),
        "hard" => return Box::new(hard()),
        _ => {}
    }

    let genome_id = if opponent == "champion" {
        store
            .get_reigning_champion()
            .ok()
            .flatten()
            .map(|c| c.genome_id)
    } else if let Some(id) = opponent.strip_prefix("museum:") {
        id.parse::<i64>().ok()
    } else {
        None
    };

    if let Some(id) = genome_id {
        if let Ok(Some(weights)) = store.get_genome_weights(id) {
            return Box::new(GenomeBot::new(weights));
        }
    }

    tracing::warn!("no genome for opponent {opponent:?}; falling back to hard bot");
    Box::new(hard())
}

/// Optional timeout override for tests/smoke runs (`CRUCIBLE_TIMEOUT_TICKS`).
fn timeout_override(mut config: GameConfig) -> GameConfig {
    if let Ok(v) = std::env::var("CRUCIBLE_TIMEOUT_TICKS") {
        if let Ok(ticks) = v.parse::<i32>() {
            config.timeout_ticks = ticks;
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_opponent_scripted_and_fallback() {
        let store = Store::in_memory().unwrap();
        assert_eq!(resolve_opponent(&store, "easy").name(), "easy");
        assert_eq!(resolve_opponent(&store, "medium").name(), "medium");
        assert_eq!(resolve_opponent(&store, "hard").name(), "hard");
        // Unknown strings and a missing champion both fall back to hard.
        assert_eq!(resolve_opponent(&store, "champion").name(), "hard");
        assert_eq!(resolve_opponent(&store, "bogus").name(), "hard");
    }

    #[test]
    fn resolve_opponent_champion_and_museum() {
        let store = Store::in_memory().unwrap();
        let weights = vec![0.1_f32, -0.2, 0.3];
        let id = store.save_genome(3, None, "init", &weights).unwrap();
        store.crown_champion(id, 3, None).unwrap();

        assert_eq!(resolve_opponent(&store, "champion").name(), "genome");
        assert_eq!(
            resolve_opponent(&store, &format!("museum:{id}")).name(),
            "genome"
        );
        // A museum id with no stored genome falls back to hard.
        assert_eq!(resolve_opponent(&store, "museum:9999").name(), "hard");
    }

    #[test]
    fn champion_opponent_plays_a_match() {
        let store = Store::in_memory().unwrap();
        let genome = crucible_ai::init(&mut crucible_sim::Rng::from_seed(7));
        let id = store.save_genome(0, None, "init", &genome).unwrap();
        store.crown_champion(id, 0, None).unwrap();

        let mut champ = resolve_opponent(&store, "champion");
        assert_eq!(champ.name(), "genome");

        // The learned commander must drive a full match through the same
        // decision layer the live WS loop uses, without panicking.
        let cfg = crucible_sim::GameConfig {
            timeout_ticks: 300,
            ..crucible_sim::GameConfig::default()
        };
        let outcome = crucible_ai::run_match(11, &cfg, &mut *champ, &mut hard());
        assert!(outcome.duration_ticks > 0, "champion match failed to run");
    }
}
