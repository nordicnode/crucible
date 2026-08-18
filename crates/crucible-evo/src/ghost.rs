//! Ghosts: frozen opponents reconstructed from a recorded match's input log.
//! A ghost replays one side's command stream (with entity-id remapping so its
//! build/attack references stay correct against a new opponent), plus the pool
//! policy that keeps recent/champion-beating human matches weighted higher.
//!
//! Pure — depends only on `crucible-sim`/`crucible-ai`. Immutability: the same
//! inputs always produce the same commands.

use std::collections::HashMap;

use crucible_ai::{run_match_detailed, Bot, GenomeBot};
use crucible_sim::{serialize, Command, EntityId, Game, GameConfig, Player, Replay, TimedCommand};

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
        // original match (deterministic) and sorting its entities by id.
        let game = serialize::finish_replay(replay);
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

/// Mean shaped fitness of `genome` against a set of ghosts. Each ghost plays
/// its recorded side (P0) on its own map; the genome plays P1.
pub fn ghost_fitness(genome: &[f32], ghosts: &[Ghost], config: &GameConfig) -> f32 {
    let mut total = 0.0f32;
    for ghost in ghosts {
        let seed = ghost.map_seed();
        let mut g = ghost.clone(); // fresh cursor
        let mut genome_bot = GenomeBot::new(genome.to_vec());
        let d = run_match_detailed(seed, config, &mut g, &mut genome_bot);
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
        // Trim oldest (lowest recency) first.
        while self.entries.len() > self.max_size {
            self.entries.remove(0);
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
}
