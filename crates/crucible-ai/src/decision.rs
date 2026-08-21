//! The decision layer: network output scores -> concrete, valid commands.
//!
//! Illegal actions are masked (set to `-inf`) before argmax, and any action
//! whose winning score is at/below the threshold is skipped. Ties break to the
//! lowest index, so the mapping is fully deterministic given genome + state.
//! The returned commands still pass through the sim's normal validator.

use crucible_sim::{
    fixed::{dist2, Pos},
    unit_stats, BuildingType, Command, EntityId, Game, Player, Stance, UnitType, Upgrade,
};

use crate::features::{extract, FeatureInput};
use crate::network::{
    forward, ARMY_ACTION_OUT, BUILD_OUT, SECTOR_OUT, SNIPE_OUT, TECH_OUT, TRAIN_OUT,
};
use crate::scripted::{combat_unit_ids, find_build_tile};

const BUILD_TYPES: [BuildingType; BUILD_OUT] = [
    BuildingType::Refinery,
    BuildingType::Barracks,
    BuildingType::Factory,
    BuildingType::TechLab,
    BuildingType::Turret,
    BuildingType::Airfield,
    BuildingType::Radar,
    BuildingType::TeslaCoil,
];
const TRAIN_TYPES: [UnitType; TRAIN_OUT] = [
    UnitType::Harvester,
    UnitType::Infantry,
    UnitType::Tank,
    UnitType::Artillery,
    UnitType::MammothTank,
    UnitType::Gunship,
    UnitType::Interceptor,
];
const TECH_TYPES: [Upgrade; TECH_OUT] =
    [Upgrade::None, Upgrade::Damage, Upgrade::Hp, Upgrade::Range];

/// Actions only fire when their winning score clears this threshold.
const THRESHOLD: f32 = 0.0;

/// Whether the army head should act at all this tick.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArmyAction {
    Attack,
    Defend,
    Scout,
    /// Focus-fire the highest-priority visible enemy of the snipe-target type
    /// chosen by the snipe head (the `Attack` command, not an attack-move).
    Snipe,
}

/// Snipe-target types, in output-slot order (see `SNIPE_OUT`). The network
/// scores each; the decision layer then picks the currently-visible enemy of
/// the winning type that is closest to the army.
const SNIPE_TYPES: [SnipeTarget; SNIPE_OUT] = [
    SnipeTarget::Unit(UnitType::Harvester),
    SnipeTarget::Building(BuildingType::Refinery),
    SnipeTarget::Building(BuildingType::Hq),
    SnipeTarget::Building(BuildingType::Factory),
];

#[derive(Clone, Copy)]
enum SnipeTarget {
    Unit(UnitType),
    Building(BuildingType),
}

/// Decide commands for `player` from a genome, a legal observation, and the
/// history embedding (the previous command ticks' feature vectors, oldest
/// first; see `features::extract`). Callers own the buffer — [`GenomeBot`]
/// maintains it across command ticks.
pub fn decide(
    game: &Game,
    player: Player,
    genome: &[f32],
    input: &FeatureInput,
    history: &[Vec<f32>],
) -> Vec<Command> {
    let feats = extract(input, history);
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

    // --- Power management (rule, not learned) ------------------------------
    // The learned build head has no PowerPlant slot, so a deterministic rule
    // keeps the AI from being permanently crippled by low power: production
    // runs at half speed once consumption exceeds production, and humans can
    // escape that by building a PowerPlant. The AI gets the same escape hatch.
    // Fires only when one more plant closes the gap, so it never over-spams.
    if game.has_low_power(player) {
        let (prod, cons) = game.power(player);
        if prod + crucible_sim::building_stats(BuildingType::PowerPlant).power >= cons
            && build_allowed(game, player, BuildingType::PowerPlant)
        {
            if let Some(pref) = build_preferred(game, player, BuildingType::PowerPlant) {
                if let Some(tile) = find_build_tile(game, player, BuildingType::PowerPlant, pref) {
                    cmds.push(Command::PlaceBuilding {
                        player,
                        btype: BuildingType::PowerPlant,
                        tile,
                    });
                }
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
        if let Some(b) = game.buildings.iter().find(|b| {
            b.owner == player && b.btype == producer && b.queue.len() < game.config.max_queue
        }) {
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
                2 => ArmyAction::Scout,
                _ => ArmyAction::Snipe,
            });
        }
    }
    let sector = argmax(&out[sector_base..sector_base + SECTOR_OUT]);

    if let Some(action) = action {
        let units = combat_unit_ids(game, player);
        if !units.is_empty() {
            let snipe_base = sector_base + SECTOR_OUT + TECH_OUT;
            let target = match action {
                ArmyAction::Defend => Some(input.own_hq_tile),
                ArmyAction::Scout => Some(sector_center(sector, input.own_hq_tile)),
                ArmyAction::Attack => Some(if units.len() >= 3 {
                    (63 - input.own_hq_tile.0, 63 - input.own_hq_tile.1)
                } else {
                    input.own_hq_tile
                }),
                // Focus-fire: pick the target type the snipe head scores highest
                // and lock the army onto the best currently-visible enemy of
                // that type. If none is visible right now, the snipe skips
                // (the features encode visibility, so the network learns when
                // a snipe can actually land).
                ArmyAction::Snipe => {
                    let kind = SNIPE_TYPES[argmax(&out[snipe_base..snipe_base + SNIPE_OUT])];
                    if let Some((target, _)) = snipe_target(game, player, input, kind) {
                        cmds.push(Command::Attack {
                            player,
                            units: units.clone(),
                            target,
                        });
                    }
                    None
                }
            };
            if let Some(waypoint) = target {
                cmds.push(Command::MoveGroup {
                    player,
                    units,
                    waypoint,
                    stance: Stance::Aggressive,
                });
            }
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

/// The best currently-visible enemy of `kind` for a focus-fire order: the one
/// closest to the army's centroid (tie → lowest id). Only enemies seen this
/// exact tick are eligible — `last_seen == tick` is the fog invariant that the
/// target is alive and enemy right now, so the emitted `Attack` command always
/// passes the sim's validator.
fn snipe_target(
    game: &Game,
    player: Player,
    input: &FeatureInput,
    kind: SnipeTarget,
) -> Option<(EntityId, Pos)> {
    let tick = game.tick;
    // Army centroid (fallback: own HQ if the army is empty/undefined).
    let units = combat_unit_ids(game, player);
    let (cx, cy) = if units.is_empty() {
        let hq = input.own_hq_tile;
        (Pos::from_tile(hq.0, hq.1).x, Pos::from_tile(hq.0, hq.1).y)
    } else {
        let mut sx = 0i64;
        let mut sy = 0i64;
        let mut n = 0i64;
        for id in units {
            if let Some(u) = game.unit(player, id) {
                sx += u.pos.x as i64;
                sy += u.pos.y as i64;
                n += 1;
            }
        }
        if n == 0 {
            return None;
        }
        ((sx / n) as i32, (sy / n) as i32)
    };

    let mut best: Option<(i64, EntityId)> = None; // (dist2, id)
    match kind {
        SnipeTarget::Unit(ut) => {
            for m in &input.fog.units {
                if m.utype == ut && m.last_seen == tick {
                    let d = dist2(cx, cy, m.pos.x, m.pos.y);
                    if best.is_none_or(|(bd, bid)| d < bd || (d == bd && m.id < bid)) {
                        best = Some((d, m.id));
                    }
                }
            }
        }
        SnipeTarget::Building(bt) => {
            for m in &input.fog.buildings {
                if m.btype == bt && m.last_seen == tick {
                    let pos = Pos::from_tile(m.tile.0, m.tile.1);
                    let d = dist2(cx, cy, pos.x, pos.y);
                    if best.is_none_or(|(bd, bid)| d < bd || (d == bd && m.id < bid)) {
                        best = Some((d, m.id));
                    }
                }
            }
        }
    }
    best.map(|(_, id)| {
        let pos = if let SnipeTarget::Unit(_) = kind {
            input
                .fog
                .units
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.pos)
                .unwrap_or_else(|| Pos::from_tile(input.own_hq_tile.0, input.own_hq_tile.1))
        } else {
            input
                .fog
                .buildings
                .iter()
                .find(|m| m.id == id)
                .map(|m| Pos::from_tile(m.tile.0, m.tile.1))
                .unwrap_or_else(|| Pos::from_tile(input.own_hq_tile.0, input.own_hq_tile.1))
        };
        (id, pos)
    })
}

fn sector_center(sector: usize, own_hq: (u8, u8)) -> (u8, u8) {
    let mut sx = sector % 8;
    let mut sy = sector / 8;
    if own_hq.0 >= 32 {
        sx = 7 - sx;
    }
    if own_hq.1 >= 32 {
        sy = 7 - sy;
    }
    ((sx * 8 + 4) as u8, (sy * 8 + 4) as u8)
}

// --- Legality masks ---------------------------------------------------------

fn build_allowed(game: &Game, p: Player, bt: BuildingType) -> bool {
    if bt != BuildingType::Refinery
        && !game
            .buildings
            .iter()
            .any(|b| b.owner == p && b.btype == BuildingType::Refinery)
    {
        return false;
    }
    if (bt == BuildingType::TechLab || bt == BuildingType::Airfield)
        && !game
            .buildings
            .iter()
            .any(|b| b.owner == p && b.btype == BuildingType::Factory)
    {
        return false;
    }
    // Second-tier structures need the TechLab itself.
    if (bt == BuildingType::Radar || bt == BuildingType::TeslaCoil)
        && !game
            .buildings
            .iter()
            .any(|b| b.owner == p && b.btype == BuildingType::TechLab)
    {
        return false;
    }
    let cost = crucible_sim::building_stats(bt).cost;
    if game.ore[p.index()] < cost {
        return false;
    }
    build_preferred(game, p, bt).is_some_and(|pref| find_build_tile(game, p, bt, pref).is_some())
}

fn base_offset(hq: (u8, u8), dx: i32, dy: i32) -> (u8, u8) {
    let sx = if hq.0 < 32 { dx } else { -dx };
    let sy = if hq.1 < 32 { dy } else { -dy };
    (
        (hq.0 as i32 + sx).clamp(0, 63) as u8,
        (hq.1 as i32 + sy).clamp(0, 63) as u8,
    )
}

fn build_preferred(game: &Game, p: Player, bt: BuildingType) -> Option<(u8, u8)> {
    let hq = game.hq(p)?;
    let hq_tile = hq.tile;
    Some(match bt {
        BuildingType::PowerPlant => base_offset(hq_tile, 0, -2),
        BuildingType::Refinery => base_offset(hq_tile, 2, 0),
        BuildingType::Factory => base_offset(hq_tile, 0, 2),
        BuildingType::Barracks => base_offset(hq_tile, 2, 2),
        BuildingType::TechLab => base_offset(hq_tile, -2, 2),
        BuildingType::Airfield => base_offset(hq_tile, -2, -2),
        BuildingType::Radar => base_offset(hq_tile, -4, 2),
        BuildingType::TeslaCoil => base_offset(hq_tile, 2, -2),
        BuildingType::Turret => base_offset(hq_tile, -2, 0),
        BuildingType::Hq => hq_tile,
    })
}

fn train_allowed(game: &Game, p: Player, ut: UnitType) -> bool {
    if (ut == UnitType::Artillery || ut == UnitType::MammothTank)
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
        UnitType::Harvester | UnitType::Tank | UnitType::Artillery | UnitType::MammothTank => {
            BuildingType::Factory
        }
        UnitType::Infantry => BuildingType::Barracks,
        UnitType::Gunship | UnitType::Interceptor => BuildingType::Airfield,
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
    use crucible_sim::fixed::Pos;
    use crucible_sim::{Game, GameConfig, Map, Rng, Unit};

    #[test]
    fn snipe_target_picks_visible_enemy_only() {
        use crucible_sim::entity::{UnitOrder, UnitType};

        let mut g = Game::new(crucible_sim::open_test_map(1), GameConfig::default());
        // P0's HQ sits at (53,53) with 5-tile vision. Place a P1 harvester
        // right beside it (visible) and a P1 tank far away (never seen).
        let spawn = |g: &mut Game, utype: UnitType, tile: (u8, u8)| {
            let stats = unit_stats(utype);
            let id = g.alloc_id();
            g.units.push(Unit {
                id,
                owner: Player::P1,
                utype,
                pos: Pos::from_tile(tile.0, tile.1),
                hp: stats.hp,
                max_hp: stats.hp,
                stance: Stance::Aggressive,
                order: UnitOrder::Idle,
                carrying: 0,
                cooldown: 0,
                park_ticks: 0,
                path: Vec::new(),
                target: None,
                fleeing: false,
                harvest_tile: None,
                refinery: None,
            });
            id
        };
        let harvester = spawn(&mut g, UnitType::Harvester, (51, 53));
        let _far_tank = spawn(&mut g, UnitType::Tank, (10, 10));
        // Step once so the fog records the visible harvester (last_seen = 1).
        g.step();

        let input = FeatureInput::from_game(&g, Player::P0);
        // The visible harvester is the only candidate of its type.
        let hit = snipe_target(
            &g,
            Player::P0,
            &input,
            SnipeTarget::Unit(UnitType::Harvester),
        );
        assert_eq!(hit.map(|(id, _)| id), Some(harvester));

        // No refinery exists (and none is visible): the snipe must skip.
        let none = snipe_target(
            &g,
            Player::P0,
            &input,
            SnipeTarget::Building(BuildingType::Refinery),
        );
        assert!(none.is_none());

        // A target that was seen but then vanished must not be picked: after
        // the harvester dies, `last_seen == tick` no longer holds (its last
        // tile stays visible, so the fog drops it rather than carrying it).
        let hidx = g.units.iter().position(|u| u.id == harvester).unwrap();
        g.units[hidx].hp = 0;
        g.step(); // sweeps the dead unit, then updates fog
        let input = FeatureInput::from_game(&g, Player::P0);
        let gone = snipe_target(
            &g,
            Player::P0,
            &input,
            SnipeTarget::Unit(UnitType::Harvester),
        );
        assert!(gone.is_none(), "a dead target must never be focus-fired");
    }

    #[test]
    fn decide_is_deterministic_and_valid() {
        let mut g = Game::new(Map::generate(5), GameConfig::default());
        for _ in 0..200 {
            g.step();
        }
        let mut rng = Rng::from_seed(1);
        let genome = init(&mut rng);
        let input = FeatureInput::from_game(&g, Player::P0);
        let a = decide(&g, Player::P0, &genome, &input, &[]);
        let b = decide(&g, Player::P0, &genome, &input, &[]);
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
        let cmds = decide(&g, Player::P0, &genome, &input, &[]);
        for cmd in &cmds {
            assert!(!matches!(
                cmd,
                Command::PlaceBuilding { .. } | Command::TrainUnit { .. }
            ));
        }
    }

    #[test]
    fn air_power_actions_are_learnable() {
        // The learned policy must be able to reach the air power actions: the
        // Airfield build slot is legal once a Refinery + Factory exist, and
        // both aircraft train slots are legal once an Airfield exists. Masking
        // must never permanently hide them from the network.
        let mut g = Game::new(Map::generate(5), GameConfig::default());
        g.ore[0] = 10_000;
        let hq = g.hq(Player::P0).unwrap().tile;

        // Refinery + Factory (the Factory gate for Airfield).
        for (bt, tile) in [
            (BuildingType::Refinery, (hq.0 + 2, hq.1)),
            (BuildingType::Factory, (hq.0, hq.1 + 2)),
        ] {
            let cmd = Command::PlaceBuilding {
                player: Player::P0,
                btype: bt,
                tile,
            };
            assert!(g.validate_command(&cmd).is_ok(), "{cmd:?} must be legal");
            g.apply_commands(Player::P0, &[cmd]);
        }
        assert!(build_allowed(&g, Player::P0, BuildingType::Airfield));

        // Place the Airfield and verify both aircraft trains are unmasked.
        let cmd = Command::PlaceBuilding {
            player: Player::P0,
            btype: BuildingType::Airfield,
            tile: (hq.0 + 2, hq.1 + 2),
        };
        assert!(g.validate_command(&cmd).is_ok(), "{cmd:?} must be legal");
        g.apply_commands(Player::P0, &[cmd]);
        assert!(train_allowed(&g, Player::P0, UnitType::Gunship));
        assert!(train_allowed(&g, Player::P0, UnitType::Interceptor));

        // The produced units actually come from the Airfield.
        assert_eq!(producer_for(UnitType::Gunship), BuildingType::Airfield);
        assert_eq!(producer_for(UnitType::Interceptor), BuildingType::Airfield);
    }

    #[test]
    fn tech_tree_actions_are_learnable() {
        // The second tier (Radar / TeslaCoil buildings, MammothTank, and the
        // Range research) must never be permanently masked once its
        // prerequisites exist, so the network can learn the whole tree.
        let mut g = Game::new(Map::generate(5), GameConfig::default());
        g.ore[0] = 100_000;
        let hq = g.hq(Player::P0).unwrap().tile;

        // Locked before the TechLab exists.
        assert!(!build_allowed(&g, Player::P0, BuildingType::Radar));
        assert!(!build_allowed(&g, Player::P0, BuildingType::TeslaCoil));
        assert!(!train_allowed(&g, Player::P0, UnitType::MammothTank));

        for (bt, tile) in [
            (BuildingType::Refinery, (hq.0 + 2, hq.1)),
            (BuildingType::Factory, (hq.0, hq.1 + 2)),
        ] {
            let cmd = Command::PlaceBuilding {
                player: Player::P0,
                btype: bt,
                tile,
            };
            assert!(g.validate_command(&cmd).is_ok(), "{cmd:?} must be legal");
            g.apply_commands(Player::P0, &[cmd]);
        }
        let cmd = Command::PlaceBuilding {
            player: Player::P0,
            btype: BuildingType::TechLab,
            tile: (hq.0 + 2, hq.1 + 2),
        };
        assert!(g.validate_command(&cmd).is_ok(), "{cmd:?} must be legal");
        g.apply_commands(Player::P0, &[cmd]);

        // Everything on the second tier is now unmasked.
        assert!(build_allowed(&g, Player::P0, BuildingType::Radar));
        assert!(build_allowed(&g, Player::P0, BuildingType::TeslaCoil));
        assert!(train_allowed(&g, Player::P0, UnitType::MammothTank));
        // MammothTank trains from the Factory.
        assert_eq!(producer_for(UnitType::MammothTank), BuildingType::Factory);
        // All three research options are reachable before an upgrade is chosen.
        for up in [Upgrade::Damage, Upgrade::Hp, Upgrade::Range] {
            assert!(tech_allowed(&g, Player::P0, up));
        }
    }
}
