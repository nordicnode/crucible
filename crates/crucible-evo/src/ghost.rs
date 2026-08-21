//! Ghosts: frozen opponents reconstructed from a recorded match's input log.
//! A ghost replays one side's command stream (with entity-id remapping so its
//! build/attack references stay correct against a new opponent), plus the pool
//! policy that keeps recent/champion-beating human matches weighted higher.
//!
//! Pure — depends only on `crucible-sim`/`crucible-ai`. Immutability: the same
//! inputs always produce the same commands.

use std::collections::{HashMap, HashSet};

use crucible_ai::{Bot, DetailedOutcome, GenomeBot, MatchOutcome};
use crucible_sim::{Command, EntityId, Game, GameConfig, Map, Player, Replay, TimedCommand};

use crate::fitness::shaped_fitness;

/// A frozen policy reconstructed from a replay. Deterministic and immutable.
#[derive(Clone, Debug)]
pub struct Ghost {
    map_seed: u64,
    commands: Vec<TimedCommand>,
    /// Original entity id -> creation-order index among the ghost's entities.
    id_to_index: HashMap<EntityId, usize>,
    cursor: usize,
}

impl Ghost {
    /// Build a ghost that replays `player`'s command stream from `replay` on
    /// the replay's own map. Entity references are remapped by creation order
    /// so the ghost stays coherent against a different opponent.
    pub fn from_replay(replay: &Replay, player: Player) -> Ghost {
        let mut commands: Vec<TimedCommand> = replay
            .commands
            .iter()
            .filter(|tc| tc.player == player)
            .cloned()
            .collect();
        commands.sort_by_key(|tc| tc.tick);

        // Reconstruct the ghost's entity creation order by re-running the
        // original match and unioning the ids of *every* entity the ghost
        // ever created — survivors and those later destroyed or sold. Ids are
        // allocated strictly ascending, so sorting the union yields creation
        // order, which is the order a fresh match creates the ghost's
        // entities too. (A survivors-only snapshot was wrong: commands
        // referencing units that died in the original match were dropped, and
        // survivors were mapped to the wrong creation rank whenever the two
        // matches' live sets differed.)
        let mut game = Game::new(Map::generate(replay.map_seed), replay.config.clone());
        let mut created: HashSet<EntityId> = HashSet::new();
        {
            let mut capture = |g: &Game| {
                for u in g.units.iter().filter(|u| u.owner == player) {
                    created.insert(u.id);
                }
                for b in g.buildings.iter().filter(|b| b.owner == player) {
                    created.insert(b.id);
                }
            };
            // Step the full match exactly as `serialize::replay_to_game` does,
            // capturing after every command application and every tick. (An
            // entity spawned and killed within a single tick could in theory
            // be missed; ids are never reused, so the worst case is one
            // missing creation slot for that extremely rare event.)
            capture(&game);
            let mut applied = 0usize;
            while applied < replay.commands.len() {
                let cmd = &replay.commands[applied];
                while game.tick < cmd.tick && !game.is_over() {
                    game.step();
                    capture(&game);
                }
                if !game.is_over() {
                    game.apply_commands(cmd.player, std::slice::from_ref(&cmd.command));
                    capture(&game);
                }
                applied += 1;
            }
        }
        let mut ids: Vec<EntityId> = created.into_iter().collect();
        ids.sort_unstable();
        let id_to_index: HashMap<EntityId, usize> =
            ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();

        Ghost {
            map_seed: replay.map_seed,
            commands,
            id_to_index,
            cursor: 0,
        }
    }

    pub fn map_seed(&self) -> u64 {
        self.map_seed
    }

    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    /// The ghost's own entities in creation order (sorted by id).
    fn current_entities(&self, game: &Game, player: Player) -> Vec<EntityId> {
        let mut ids: Vec<EntityId> = game
            .units
            .iter()
            .filter(|u| u.owner == player)
            .map(|u| u.id)
            .chain(
                game.buildings
                    .iter()
                    .filter(|b| b.owner == player)
                    .map(|b| b.id),
            )
            .collect();
        ids.sort_unstable();
        ids
    }

    fn remap(&self, cmd: &Command, entities: &[EntityId], player: Player) -> Option<Command> {
        use Command::*;
        let at = |id: &EntityId| {
            self.id_to_index
                .get(id)
                .and_then(|&k| entities.get(k).copied())
        };
        match cmd {
            PlaceBuilding { btype, tile, .. } => Some(PlaceBuilding {
                player,
                btype: *btype,
                tile: *tile,
            }),
            TrainUnit {
                building, utype, ..
            } => Some(TrainUnit {
                player,
                building: at(building)?,
                utype: *utype,
            }),
            MoveGroup {
                units,
                waypoint,
                stance,
                ..
            } => {
                let mut new_units = Vec::with_capacity(units.len());
                for id in units {
                    new_units.push(at(id)?);
                }
                Some(MoveGroup {
                    player,
                    units: new_units,
                    waypoint: *waypoint,
                    stance: *stance,
                })
            }
            Attack { units, target, .. } => {
                let mut new_units = Vec::with_capacity(units.len());
                for id in units {
                    new_units.push(at(id)?);
                }
                Some(Attack {
                    player,
                    units: new_units,
                    target: at(target)?,
                })
            }
            SetRally {
                building, waypoint, ..
            } => Some(SetRally {
                player,
                building: at(building)?,
                waypoint: *waypoint,
            }),
            ChooseUpgrade { lab, upgrade, .. } => Some(ChooseUpgrade {
                player,
                lab: at(lab)?,
                upgrade: *upgrade,
            }),
            Sell { building, .. } => Some(Sell {
                player,
                building: at(building)?,
            }),
            Repair { building, .. } => Some(Repair {
                player,
                building: at(building)?,
            }),
        }
    }
}

impl Bot for Ghost {
    fn name(&self) -> &'static str {
        "ghost"
    }

    fn decide(&mut self, game: &Game, player: Player) -> Vec<Command> {
        let mut out = Vec::new();
        let entities = self.current_entities(game, player);
        while self.cursor < self.commands.len() {
            let tc = &self.commands[self.cursor];
            if tc.tick > game.tick {
                break;
            }
            if tc.tick == game.tick {
                if let Some(cmd) = self.remap(&tc.command, &entities, player) {
                    out.push(cmd);
                }
            }
            self.cursor += 1;
        }
        out
    }
}

/// Run one match: the ghost plays its recorded side (P0), a bot plays P1.
///
/// The ghost is polled **every tick**, not just on command ticks: human
/// commands are recorded at the tick they arrive (which can fall between the
/// 2 s command-tick boundaries), and the ghost fires them only when its
/// cursor reaches that exact tick. The opponent bot keeps the command-tick
/// cadence like any other AI.
fn run_ghost_match(ghost: &mut Ghost, bot: &mut dyn Bot, config: &GameConfig) -> DetailedOutcome {
    let mut game = Game::new(Map::generate(ghost.map_seed()), config.clone());
    // Deadlock guard only: an unlimited config must not truncate the ghost's
    // recorded command stream at ~100 s.
    let max_ticks = if config.timeout_ticks > 0 {
        config.timeout_ticks + 1_000
    } else {
        1_000_000
    };
    while !game.is_over() && game.tick < max_ticks {
        let ghost_cmds = ghost.decide(&game, Player::P0);
        game.apply_commands(Player::P0, &ghost_cmds);
        if game.is_command_tick() {
            let bot_cmds = bot.decide(&game, Player::P1);
            game.apply_commands(Player::P1, &bot_cmds);
        }
        game.step();
    }
    DetailedOutcome {
        outcome: MatchOutcome {
            winner: game.winner,
            reason: game.win_reason,
            duration_ticks: game.tick,
        },
        p0_value: game.remaining_value(Player::P0),
        p1_value: game.remaining_value(Player::P1),
    }
}

/// Mean shaped fitness of `genome` against a set of ghosts. Each ghost plays
/// its recorded side (P0) on its own map; the genome plays P1.
pub fn ghost_fitness(genome: &[f32], ghosts: &[Ghost], config: &GameConfig) -> f32 {
    let mut total = 0.0f32;
    for ghost in ghosts {
        let mut g = ghost.clone(); // fresh cursor
        let mut genome_bot = GenomeBot::new(genome.to_vec());
        let d = run_ghost_match(&mut g, &mut genome_bot, config);
        total += shaped_fitness(&d, Player::P1);
    }
    total / ghosts.len().max(1) as f32
}

/// A ghost in the pool plus its metadata.
#[derive(Clone, Debug)]
pub struct GhostEntry {
    pub id: u64,
    pub ghost: Ghost,
    pub beat_champion: bool,
    pub recency: u64,
}

/// The ghost pool: recent human matches weighted higher, champion-beaters
/// retained, trimmed to a maximum size.
#[derive(Clone, Debug, Default)]
pub struct GhostPool {
    entries: Vec<GhostEntry>,
    max_size: usize,
    next_recency: u64,
}

impl GhostPool {
    pub fn new(max_size: usize) -> Self {
        GhostPool {
            entries: Vec::new(),
            max_size,
            next_recency: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn add(&mut self, id: u64, ghost: Ghost, beat_champion: bool) {
        self.entries.push(GhostEntry {
            id,
            ghost,
            beat_champion,
            recency: self.next_recency,
        });
        self.next_recency += 1;
        // Trim to `max_size`, evicting the oldest **non-beater** first: the
        // pool policy promises champion-beaters are retained, so a burst of
        // ordinary matches must not push out the one strategy that beat the
        // champion. Only when the pool is entirely beaters does the oldest
        // beater give way.
        while self.entries.len() > self.max_size {
            let evict = self
                .entries
                .iter()
                .position(|e| !e.beat_champion)
                .unwrap_or(0);
            self.entries.remove(evict);
        }
    }

    /// Champion-beating ghosts, most recent first.
    pub fn champion_beaters(&self) -> Vec<Ghost> {
        let mut v: Vec<&GhostEntry> = self.entries.iter().filter(|e| e.beat_champion).collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.recency));
        v.into_iter().map(|e| e.ghost.clone()).collect()
    }

    /// Sample up to `n` ghosts, weighted by recency (recent = higher weight),
    /// without replacement.
    pub fn sample(&self, rng: &mut crucible_sim::Rng, n: usize) -> Vec<Ghost> {
        let n = n.min(self.entries.len());
        if n == 0 {
            return Vec::new();
        }
        let mut idx: Vec<usize> = (0..self.entries.len()).collect();
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            let total: u64 = idx.iter().map(|&i| self.entries[i].recency + 1).sum();
            let mut pick = rng.below(total);
            let mut chosen = 0usize;
            for (pos, &i) in idx.iter().enumerate() {
                let w = self.entries[i].recency + 1;
                if pick < w {
                    chosen = pos;
                    break;
                }
                pick -= w;
            }
            out.push(self.entries[idx[chosen]].ghost.clone());
            idx.remove(chosen);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_replay(seed: u64) -> Replay {
        // Record a short human-vs-noop match (the "human" is the easy bot).
        let cfg = GameConfig {
            timeout_ticks: 400,
            ..GameConfig::default()
        };
        let mut replay = Replay::new(seed, cfg.clone());
        let mut game = crucible_sim::Game::new(crucible_sim::Map::generate(seed), cfg);
        let mut bot = crucible_ai::easy();
        while !game.is_over() {
            if game.is_command_tick() {
                let cmds = bot.decide(&game, Player::P0);
                for c in &cmds {
                    replay.record(game.tick, Player::P0, c.clone());
                }
                game.apply_commands(Player::P0, &cmds);
            }
            game.step();
        }
        replay
    }

    #[test]
    fn ghost_is_immutable_and_deterministic() {
        let replay = sample_replay(11);
        let ghost = Ghost::from_replay(&replay, Player::P0);
        assert_eq!(ghost.command_count(), replay.commands.len());

        // Replaying on the same map reproduces the same command stream.
        let mut g1 = ghost.clone();
        let mut g2 = ghost.clone();
        let mut game = crucible_sim::Game::new(
            crucible_sim::Map::generate(replay.map_seed),
            replay.config.clone(),
        );
        // Step to a couple of command ticks and compare outputs.
        while game.tick < 120 && !game.is_over() {
            if game.is_command_tick() {
                let a = g1.decide(&game, Player::P0);
                let b = g2.decide(&game, Player::P0);
                assert_eq!(a, b, "ghost diverged at tick {}", game.tick);
            }
            game.step();
        }
    }

    #[test]
    fn pool_keeps_beaters_and_trims_oldest() {
        let replay = sample_replay(3);
        let mut pool = GhostPool::new(3);
        let g = || Ghost::from_replay(&replay, Player::P0);
        pool.add(1, g(), false);
        pool.add(2, g(), true); // beat the champion
        pool.add(3, g(), false);
        pool.add(4, g(), false); // pushes out entry 1 (max 3)

        assert_eq!(pool.len(), 3);
        assert_eq!(pool.champion_beaters().len(), 1);

        // Sampling is deterministic and never exceeds the requested count.
        let mut rng = crucible_sim::Rng::from_seed(7);
        let a = pool.sample(&mut rng, 2);
        let mut rng2 = crucible_sim::Rng::from_seed(7);
        let b = pool.sample(&mut rng2, 2);
        assert_eq!(a.len(), 2);
        assert!(a[0].command_count() > 0);
        // Same seed ⇒ same sample.
        assert_eq!(a.len(), b.len());
        assert_eq!(a[0].map_seed(), b[0].map_seed());
    }

    #[test]
    fn champion_beaters_survive_recency_eviction() {
        // The pool must retain champion-beaters even when ordinary matches
        // flood in afterwards; only an all-beater pool evicts its oldest.
        let replay = sample_replay(4);
        let g = || Ghost::from_replay(&replay, Player::P0);
        let mut pool = GhostPool::new(2);
        pool.add(1, g(), true); // champion-beater
        pool.add(2, g(), false);
        pool.add(3, g(), false); // ordinary match: evicts entry 2, not the beater
        assert_eq!(pool.len(), 2);
        assert_eq!(pool.champion_beaters().len(), 1);

        // A flood of ordinary matches still cannot evict the beater.
        for id in 4..=50u64 {
            pool.add(id, g(), false);
        }
        assert_eq!(pool.len(), 2);
        assert_eq!(pool.champion_beaters().len(), 1);
        assert_eq!(
            pool.champion_beaters()[0].command_count(),
            g().command_count()
        );

        // An all-beater pool trims its oldest beater (size still respected).
        let mut all = GhostPool::new(2);
        all.add(1, g(), true);
        all.add(2, g(), true);
        all.add(3, g(), true);
        assert_eq!(all.len(), 2);
        assert_eq!(all.champion_beaters().len(), 2);
    }

    #[test]
    fn ghost_fitness_is_deterministic() {
        let replay = sample_replay(5);
        let ghost = Ghost::from_replay(&replay, Player::P0);
        let genome = crucible_ai::init(&mut crucible_sim::Rng::from_seed(9));
        let cfg = GameConfig {
            timeout_ticks: 300,
            ..GameConfig::default()
        };
        let a = ghost_fitness(&genome, std::slice::from_ref(&ghost), &cfg);
        let b = ghost_fitness(&genome, &[ghost], &cfg);
        assert_eq!(a, b);
        assert!(a.is_finite());
    }

    struct NoopBot;
    impl crucible_ai::Bot for NoopBot {
        fn name(&self) -> &'static str {
            "noop"
        }
        fn decide(&mut self, _g: &Game, _p: Player) -> Vec<Command> {
            Vec::new()
        }
    }

    #[test]
    fn ghost_replays_mid_tick_commands() {
        // A human command issued between command ticks (tick 21, with
        // COMMAND_TICK = 20) must fire exactly when the ghost's cursor reaches
        // it. The live server now applies human commands on arrival, so the
        // ghost runner polls every tick — a command-tick-only poll would drop
        // this command.
        let cfg = GameConfig {
            timeout_ticks: 400,
            ..GameConfig::default()
        };
        let seed = 42u64;
        let mut replay = Replay::new(seed, cfg.clone());
        let mut game = Game::new(Map::generate(seed), cfg.clone());
        while game.tick < 21 && !game.is_over() {
            game.step();
        }
        let hq = game.hq(Player::P0).unwrap().tile;
        let cmd = Command::PlaceBuilding {
            player: Player::P0,
            btype: crucible_sim::BuildingType::Refinery,
            tile: (hq.0 + 2, hq.1),
        };
        replay.record(21, Player::P0, cmd.clone());
        game.apply_commands(Player::P0, &[cmd]); // Drive the ghost with the same every-tick polling run_ghost_match uses.
        let mut ghost = Ghost::from_replay(&replay, Player::P0);
        let mut game = Game::new(Map::generate(seed), cfg.clone());
        while game.tick < 22 && !game.is_over() {
            let cmds = ghost.decide(&game, Player::P0);
            game.apply_commands(Player::P0, &cmds);
            game.step();
        }
        assert!(
            game.buildings
                .iter()
                .any(|b| b.owner == Player::P0 && b.btype == crucible_sim::BuildingType::Refinery),
            "ghost dropped the mid-tick refinery command"
        );

        // The production runner applies the same every-tick polling and
        // completes against a noop opponent; P0 wins the timeout by value
        // (the refinery tips the otherwise-tied bases).
        let mut ghost = Ghost::from_replay(&replay, Player::P0);
        let outcome = run_ghost_match(&mut ghost, &mut NoopBot, &cfg);
        assert!(outcome.outcome.duration_ticks > 21);
        assert_eq!(outcome.outcome.winner, Some(Player::P0));
        assert!(outcome.p0_value > outcome.p1_value);
    }

    #[test]
    fn ghost_maps_entities_by_creation_order_not_survivors() {
        // Drive a hard-vs-hard match so the ghost side (P0) suffers
        // casualties, then verify the ghost can replay *every* recorded
        // command against a fresh, byte-identical match. The old survivor-only
        // mapping dropped commands whose target died in the original match and
        // mis-ranked survivors whenever the two matches' live sets differed.
        let cfg = GameConfig {
            timeout_ticks: 900,
            ..GameConfig::default()
        };
        let seed = 2026u64;
        let mut game = Game::new(Map::generate(seed), cfg.clone());
        let mut replay = Replay::new(seed, cfg.clone());
        let mut p0 = crucible_ai::hard();
        let mut p1 = crucible_ai::hard();
        while !game.is_over() {
            if game.is_command_tick() {
                for (bot, player) in [
                    (&mut p0 as &mut dyn crucible_ai::Bot, Player::P0),
                    (&mut p1 as &mut dyn crucible_ai::Bot, Player::P1),
                ] {
                    let cmds = bot.decide(&game, player);
                    for c in &cmds {
                        replay.record(game.tick, player, c.clone());
                    }
                    game.apply_commands(player, &cmds);
                }
            }
            game.step();
        }

        // The scenario must include P0 casualties for this to be a regression
        // test of the survivor-mapping bug.
        let p0_deaths = game
            .events
            .iter()
            .filter(|e| {
                matches!(
                    &e.kind,
                    crucible_sim::EventKind::UnitDied {
                        owner: Player::P0,
                        ..
                    }
                )
            })
            .count();
        assert!(p0_deaths > 0, "test scenario must include P0 casualties");

        let ghost = Ghost::from_replay(&replay, Player::P0);

        // Fresh match with the same seed and the same opponent is byte-identical
        // to the original, so every recorded command must still be emitted.
        let mut g = ghost.clone();
        let mut fresh = Game::new(Map::generate(replay.map_seed), replay.config.clone());
        let mut opp = crucible_ai::hard();
        let mut emitted = 0usize;
        let max_ticks = replay.config.timeout_ticks + 1_000;
        while !fresh.is_over() && fresh.tick < max_ticks {
            let cmds = g.decide(&fresh, Player::P0);
            emitted += cmds.len();
            fresh.apply_commands(Player::P0, &cmds);
            if fresh.is_command_tick() {
                let b = opp.decide(&fresh, Player::P1);
                fresh.apply_commands(Player::P1, &b);
            }
            fresh.step();
        }
        assert_eq!(
            emitted,
            ghost.command_count(),
            "ghost dropped commands during entity remapping"
        );
    }
}
