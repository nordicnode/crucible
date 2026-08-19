//! Fog-legal feature extraction: the commander's only observation of the
//! world.
//!
//! [`FeatureInput`] carries the player's *own* full state plus a
//! [`FogView`] for the enemy. It never contains the live state of hidden enemy
//! entities — that is enforced by construction, and a fuzz test asserts it.

use crucible_sim::{
    fixed::MATCH_TIMEOUT_TICKS, fog::FogView, unit_stats, BuildingType, Game, Player, UnitType,
    Upgrade,
};

/// Time window (ticks) over which remembered enemy sightings decay to zero.
pub const DECAY_TICKS: i32 = 300; // 30 seconds

/// The fixed feature-vector dimension.
pub const FEATURE_DIM: usize = 104;

/// An own building as seen by the feature extractor.
#[derive(Clone, Debug)]
pub struct OwnBuilding {
    pub btype: BuildingType,
    pub queue_len: usize,
    pub hp: i32,
    pub max_hp: i32,
}

/// The commander's legal observation. Own state is complete; enemy state comes
/// only through [`FogView`].
#[derive(Clone, Debug)]
pub struct FeatureInput {
    pub tick: i32,
    pub ore: i32,
    pub upgrade: Upgrade,
    pub own_units: Vec<UnitType>,
    pub own_buildings: Vec<OwnBuilding>,
    pub own_hq_tile: (u8, u8),
    pub fog: FogView,
}

impl FeatureInput {
    /// Build the observation for `player` from the current game.
    pub fn from_game(game: &Game, player: Player) -> Self {
        let own_units = game
            .units
            .iter()
            .filter(|u| u.owner == player && u.is_alive())
            .map(|u| u.utype)
            .collect();
        let own_buildings = game
            .buildings
            .iter()
            .filter(|b| b.owner == player && b.is_alive())
            .map(|b| OwnBuilding {
                btype: b.btype,
                queue_len: b.queue.len(),
                hp: b.hp,
                max_hp: b.max_hp,
            })
            .collect();
        let own_hq_tile = game
            .hq(player)
            .map(|b| b.tile)
            .unwrap_or(game.map.hq_tiles[player.index()]);
        FeatureInput {
            tick: game.tick,
            ore: game.ore[player.index()],
            upgrade: game.upgrades[player.index()],
            own_units,
            own_buildings,
            own_hq_tile,
            fog: game.fog_view(player),
        }
    }
}

/// Decay weight for a sighting at `last_seen` given the current tick, in [0,1].
#[inline]
fn decay(tick: i32, last_seen: i32) -> f32 {
    let age = (tick - last_seen).max(0);
    (1.0 - age as f32 / DECAY_TICKS as f32).clamp(0.0, 1.0)
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn count_own(input: &FeatureInput, ut: UnitType) -> usize {
    input.own_units.iter().filter(|&&u| u == ut).count()
}

fn count_buildings(input: &FeatureInput, bt: BuildingType) -> usize {
    input.own_buildings.iter().filter(|b| b.btype == bt).count()
}

/// Extract the fixed-size feature vector.
///
/// Layout (documented so `network.rs` and `decision.rs` stay in sync):
/// ```text
/// [0]   banked ore / 2000
/// [1..6] own building counts: refinery/4, factory/4, barracks/4, techlab/2, turret/8
/// [6]   own harvesters / 16
/// [7..10] own infantry/16, tanks/16, artillery/16
/// [10]  own army value / 2000
/// [11..15] observed enemy units (decayed): infantry, tank, artillery, harvester
/// [15..21] observed enemy buildings (decayed): hq, refinery, factory, barracks, techlab, turret
/// [21..85] enemy building presence per 8x8 sector (64), decayed, capped
/// [85]  unexplored fraction
/// [86]  reserved (0)
/// [87]  reserved (0)
/// [88]  game time / timeout
/// [89]  own HQ HP fraction
/// [90]  factory queue / 8
/// [91]  barracks queue / 8
/// [92]  idle production buildings / 8
/// [93..96] upgrade one-hot: none, damage, hp
/// [96..104] reserved (0)
/// ```
pub fn extract(input: &FeatureInput) -> Vec<f32> {
    let mut f = vec![0.0f32; FEATURE_DIM];
    let tick = input.tick;

    f[0] = clamp01(input.ore as f32 / 2000.0);

    f[1] = clamp01(count_buildings(input, BuildingType::Refinery) as f32 / 4.0);
    f[2] = clamp01(count_buildings(input, BuildingType::Factory) as f32 / 4.0);
    f[3] = clamp01(count_buildings(input, BuildingType::Barracks) as f32 / 4.0);
    f[4] = clamp01(count_buildings(input, BuildingType::TechLab) as f32 / 2.0);
    f[5] = clamp01(count_buildings(input, BuildingType::Turret) as f32 / 8.0);

    f[6] = clamp01(count_own(input, UnitType::Harvester) as f32 / 16.0);
    f[7] = clamp01(count_own(input, UnitType::Infantry) as f32 / 16.0);
    f[8] = clamp01(count_own(input, UnitType::Tank) as f32 / 16.0);
    f[9] = clamp01(count_own(input, UnitType::Artillery) as f32 / 16.0);

    let army_value: i32 = input.own_units.iter().map(|&u| unit_stats(u).cost).sum();
    f[10] = clamp01(army_value as f32 / 2000.0);

    // Observed enemy units (decayed).
    for m in &input.fog.units {
        let w = decay(tick, m.last_seen);
        let idx = match m.utype {
            UnitType::Infantry => 11,
            UnitType::Tank => 12,
            UnitType::Artillery => 13,
            UnitType::Harvester => 14,
        };
        f[idx] = clamp01(f[idx] + w / 16.0);
    }

    // Observed enemy buildings (decayed).
    for m in &input.fog.buildings {
        let w = decay(tick, m.last_seen);
        let idx = match m.btype {
            BuildingType::Hq => 15,
            BuildingType::Refinery | BuildingType::PowerPlant => 16,
            BuildingType::Factory => 17,
            BuildingType::Barracks => 18,
            BuildingType::TechLab | BuildingType::Airfield => 19,
            BuildingType::Turret => 20,
        };
        f[idx] = clamp01(f[idx] + w / 4.0);
    }

    // Enemy building presence per 8x8 sector (oriented relative to own HQ corner).
    let (flip_x, flip_y) = (input.own_hq_tile.0 >= 32, input.own_hq_tile.1 >= 32);
    for m in &input.fog.buildings {
        let mut sx = (m.tile.0 as usize) / 8;
        let mut sy = (m.tile.1 as usize) / 8;
        if flip_x {
            sx = 7 - sx;
        }
        if flip_y {
            sy = 7 - sy;
        }
        let w = decay(tick, m.last_seen);
        f[21 + sy * 8 + sx] = clamp01(f[21 + sy * 8 + sx] + w);
    }

    // [85] reserved: full explored-tile tracking lives in the sim's fog memory
    // and is deferred; hold a neutral value so the input shape stays fixed.
    f[85] = 0.5;

    f[88] = clamp01(tick as f32 / MATCH_TIMEOUT_TICKS as f32);

    // Own HQ HP fraction.
    let hq = input
        .own_buildings
        .iter()
        .find(|b| b.btype == BuildingType::Hq);
    f[89] = hq
        .map(|b| clamp01(b.hp as f32 / b.max_hp as f32))
        .unwrap_or(0.0);

    // Production queues and idle buildings.
    let mut factory_q = 0usize;
    let mut barracks_q = 0usize;
    let mut idle = 0usize;
    for b in &input.own_buildings {
        match b.btype {
            BuildingType::Factory => factory_q += b.queue_len,
            BuildingType::Barracks => barracks_q += b.queue_len,
            _ => {}
        }
        if (b.btype == BuildingType::Factory || b.btype == BuildingType::Barracks)
            && b.queue_len == 0
        {
            idle += 1;
        }
    }
    f[90] = clamp01(factory_q as f32 / 8.0);
    f[91] = clamp01(barracks_q as f32 / 8.0);
    f[92] = clamp01(idle as f32 / 8.0);

    // Upgrade one-hot.
    match input.upgrade {
        Upgrade::None => f[93] = 1.0,
        Upgrade::Damage => f[94] = 1.0,
        Upgrade::Hp => f[95] = 1.0,
    }

    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crucible_sim::{Game, GameConfig, Map};

    #[test]
    fn feature_dim_is_stable() {
        let mut g = Game::new(Map::generate(1), GameConfig::default());
        g.step();
        let input = FeatureInput::from_game(&g, Player::P0);
        let f = extract(&input);
        assert_eq!(f.len(), FEATURE_DIM);
        assert_eq!(f.len(), 104);
    }

    #[test]
    fn features_are_bounded() {
        let mut g = Game::new(Map::generate(1), GameConfig::default());
        for _ in 0..50 {
            g.step();
        }
        let f = extract(&FeatureInput::from_game(&g, Player::P0));
        for v in &f {
            assert!(v.is_finite());
            assert!((0.0..=1.0).contains(v));
        }
    }
}
