//! The decision layer: network output scores -> concrete, valid commands.
//!
//! Illegal actions are masked (set to `-inf`) before argmax, and any action
//! whose winning score is at/below the threshold is skipped. Ties break to the
//! lowest index, so the mapping is fully deterministic given genome + state.
//! The returned commands still pass through the sim's normal validator.

use crucible_sim::{unit_stats, BuildingType, Command, Game, Player, Stance, UnitType, Upgrade};

use crate::features::{extract, FeatureInput};
use crate::network::{forward, ARMY_ACTION_OUT, BUILD_OUT, SECTOR_OUT, TECH_OUT, TRAIN_OUT};
use crate::scripted::{combat_unit_ids, find_build_tile, is_valid_build_tile};

const BUILD_TYPES: [BuildingType; BUILD_OUT] = [
    BuildingType::Refinery,
    BuildingType::Barracks,
    BuildingType::Factory,
    BuildingType::TechLab,
    BuildingType::Turret,
];
const TRAIN_TYPES: [UnitType; TRAIN_OUT] = [
    UnitType::Harvester,
    UnitType::Infantry,
    UnitType::Tank,
    UnitType::Artillery,
];
const TECH_TYPES: [Upgrade; TECH_OUT] = [Upgrade::None, Upgrade::Damage, Upgrade::Hp];

/// Actions only fire when their winning score clears this threshold.
const THRESHOLD: f32 = 0.0;

/// Whether the army head should act at all this tick.
enum ArmyAction {
    Attack,
    Defend,
    Scout,
}

/// Decide commands for `player` from a genome and a legal observation.
pub fn decide(game: &Game, player: Player, genome: &[f32], input: &FeatureInput) -> Vec<Command> {
    let feats = extract(input);
    let out = forward(genome, &feats);
    let mut cmds = Vec::new();

    // --- Build head ---------------------------------------------------------
    let build_base = 0;
    let mut best_build: Option<(f32, usize)> = None;
    for i in 0..BUILD_OUT {
        let btype = BUILD_TYPES[i];
        if build_allowed(game, player, btype) {
            let s = out[build_base + i];
            if s > THRESHOLD && best_build.is_none_or(|(bs, _)| s > bs) {
                best_build = Some((s, i));
            }
        }
    }
    if let Some((_, i)) = best_build {
        let btype = BUILD_TYPES[i];
        if let Some(pref) = build_preferred(game, player, btype) {
            if let Some(tile) = find_build_tile(game, player, btype, pref) {
                cmds.push(Command::PlaceBuilding {
                    player,
                    btype,
                    tile,
                });
            }
        }
    }

    // --- Train head ---------------------------------------------------------
    let train_base = BUILD_OUT;
    let mut best_train: Option<(f32, usize)> = None;
    for i in 0..TRAIN_OUT {
        let utype = TRAIN_TYPES[i];
        if train_allowed(game, player, utype) {
            let s = out[train_base + i];
            if s > THRESHOLD && best_train.is_none_or(|(bs, _)| s > bs) {
                best_train = Some((s, i));
            }
        }
    }
    if let Some((_, i)) = best_train {
        let utype = TRAIN_TYPES[i];
        let producer = producer_for(utype);
        if let Some(b) = game
            .buildings
            .iter()
            .find(|b| b.owner == player && b.btype == producer)
        {
            cmds.push(Command::TrainUnit {
                player,
                building: b.id,
                utype,
            });
        }
    }

    // --- Army head ----------------------------------------------------------
    let action_base = BUILD_OUT + TRAIN_OUT;
    let sector_base = action_base + ARMY_ACTION_OUT;
    let mut action = None;
    let mut action_score = THRESHOLD;
    for i in 0..ARMY_ACTION_OUT {
        if out[action_base + i] > action_score {
            action_score = out[action_base + i];
            action = Some(match i {
                0 => ArmyAction::Attack,
                1 => ArmyAction::Defend,
                _ => ArmyAction::Scout,
            });
        }
    }
    let sector = argmax(&out[sector_base..sector_base + SECTOR_OUT]);

    if let Some(action) = action {
        let units = combat_unit_ids(game, player);
        if !units.is_empty() {
            let target = match action {
                ArmyAction::Defend => game.hq(player).map(|b| b.tile).unwrap_or((32, 32)),
                ArmyAction::Attack | ArmyAction::Scout => sector_center(sector),
            };
            cmds.push(Command::MoveGroup {
                player,
                units,
                waypoint: target,
                stance: Stance::Aggressive,
            });
        }
    }

    // --- Tech head ----------------------------------------------------------
    let tech_base = sector_base + SECTOR_OUT;
    let mut best_tech: Option<(f32, Upgrade)> = None;
    for i in 0..TECH_OUT {
        let up = TECH_TYPES[i];
        if up != Upgrade::None && tech_allowed(game, player, up) {
            let s = out[tech_base + i];
            if s > THRESHOLD && best_tech.is_none_or(|(bs, _)| s > bs) {
                best_tech = Some((s, up));
            }
        }
    }
    if let Some((_, up)) = best_tech {
        if let Some(lab) = game
            .buildings
            .iter()
            .find(|b| b.owner == player && b.btype == BuildingType::TechLab)
        {
            cmds.push(Command::ChooseUpgrade {
                player,
                lab: lab.id,
                upgrade: up,
            });
        }
    }

    cmds
}

fn argmax(xs: &[f32]) -> usize {
    let mut best = 0;
    for (i, &v) in xs.iter().enumerate() {
        if v > xs[best] {
            best = i;
        }
    }
    best
}

fn sector_center(sector: usize) -> (u8, u8) {
    let sx = sector % 8;
    let sy = sector / 8;
    ((sx * 8 + 4) as u8, (sy * 8 + 4) as u8)
}

// --- Legality masks ---------------------------------------------------------

fn build_allowed(game: &Game, p: Player, bt: BuildingType) -> bool {
    if bt == BuildingType::TechLab
        && !game
            .buildings
            .iter()
            .any(|b| b.owner == p && b.btype == BuildingType::Factory)
    {
        return false;
    }
    let cost = crucible_sim::building_stats(bt).cost;
    if game.ore[p.index()] < cost {
        return false;
    }
    build_preferred(game, p, bt).is_some_and(|pref| is_valid_build_tile(game, p, bt, pref))
}

fn build_preferred(game: &Game, p: Player, bt: BuildingType) -> Option<(u8, u8)> {
    let hq = game.hq(p)?;
    let (hx, hy) = hq.tile;
    Some(match bt {
        BuildingType::Refinery => (hx + 2, hy),
        BuildingType::Factory => (hx, hy + 2),
        BuildingType::Barracks => (hx + 2, hy + 2),
        BuildingType::TechLab => (hx, hy.wrapping_sub(2)),
        BuildingType::Turret => (hx.wrapping_sub(2), hy),
        BuildingType::Hq => (hx, hy),
    })
}

fn train_allowed(game: &Game, p: Player, ut: UnitType) -> bool {
    if ut == UnitType::Artillery
        && !game
            .buildings
            .iter()
            .any(|b| b.owner == p && b.btype == BuildingType::TechLab)
    {
        return false;
    }
    if game.ore[p.index()] < unit_stats(ut).cost {
        return false;
    }
    let producer = producer_for(ut);
    game.buildings
        .iter()
        .any(|b| b.owner == p && b.btype == producer && b.queue.len() < game.config.max_queue)
}

fn producer_for(ut: UnitType) -> BuildingType {
    match ut {
        UnitType::Harvester | UnitType::Tank | UnitType::Artillery => BuildingType::Factory,
        UnitType::Infantry => BuildingType::Barracks,
    }
}

fn tech_allowed(game: &Game, p: Player, up: Upgrade) -> bool {
    game.upgrades[p.index()] == Upgrade::None
        && game
            .buildings
            .iter()
            .any(|b| b.owner == p && b.btype == BuildingType::TechLab)
        && up != Upgrade::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::init;
    use crucible_sim::{Game, GameConfig, Map, Rng};

    #[test]
    fn decide_is_deterministic_and_valid() {
        let mut g = Game::new(Map::generate(5), GameConfig::default());
        for _ in 0..200 {
            g.step();
        }
        let mut rng = Rng::from_seed(1);
        let genome = init(&mut rng);
        let input = FeatureInput::from_game(&g, Player::P0);
        let a = decide(&g, Player::P0, &genome, &input);
        let b = decide(&g, Player::P0, &genome, &input);
        assert_eq!(a, b);
        // Every emitted command must pass validation.
        for cmd in &a {
            assert!(g.validate_command(cmd).is_ok(), "invalid command {cmd:?}");
        }
    }

    #[test]
    fn illegal_actions_are_masked() {
        // With zero ore and no buildings, no build/train action is possible.
        let mut g = Game::new(Map::generate(5), GameConfig::default());
        g.ore[0] = 0;
        let mut rng = Rng::from_seed(2);
        let genome = init(&mut rng);
        let input = FeatureInput::from_game(&g, Player::P0);
        let cmds = decide(&g, Player::P0, &genome, &input);
        for cmd in &cmds {
            assert!(!matches!(
                cmd,
                Command::PlaceBuilding { .. } | Command::TrainUnit { .. }
            ));
        }
    }
}
