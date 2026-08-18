//! Procedural map generation and deterministic grid pathfinding.
//!
//! Maps are generated from a `u64` seed and are **exactly point-symmetric**
//! under the reflection `(x, y) -> (63-x, 63-y)`. This makes spawn fairness a
//! theorem rather than a heuristic: every ore field, obstacle, and the two
//! HQs have identical mirror images. Generation retries with derived seeds
//! until the map is fully connected, and falls back to an open map.

use std::collections::BinaryHeap;

use serde::{Deserialize, Serialize};

use crate::fixed::{Pos, FIX_SCALE};
use crate::rng::Rng;

pub const MAP_SIZE: usize = 64;
pub const MAP_TILES: usize = MAP_SIZE * MAP_SIZE;

const MAX_GEN_ATTEMPTS: u64 = 256;

#[inline]
pub fn tile_index(x: u8, y: u8) -> usize {
    (y as usize) * MAP_SIZE + (x as usize)
}

#[inline]
pub fn tile_coords(idx: usize) -> (u8, u8) {
    ((idx % MAP_SIZE) as u8, (idx / MAP_SIZE) as u8)
}

/// Mirror a coordinate under the map's point symmetry.
#[inline]
pub fn mirror(x: u8) -> u8 {
    (MAP_SIZE - 1) as u8 - x
}

/// The static world layout. Ore amounts mutate during a match; passability
/// and positions do not.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Map {
    pub seed: u64,
    /// Passable terrain (true = walkable).
    pub passable: Vec<bool>,
    /// Ore remaining per tile.
    pub ore: Vec<i32>,
    /// HQ spawn tiles, indexed by player.
    pub hq_tiles: [(u8, u8); 2],
}

impl Map {
    pub fn generate(seed: u64) -> Map {
        for attempt in 0..MAX_GEN_ATTEMPTS {
            let s = seed.wrapping_add(attempt);
            if let Some(map) = try_generate(s) {
                return map;
            }
        }
        // Guaranteed-valid fallback: symmetric, fully open.
        open_map(seed)
    }

    #[inline]
    pub fn is_passable(&self, x: u8, y: u8) -> bool {
        self.passable[tile_index(x, y)]
    }

    #[inline]
    pub fn ore_at(&self, x: u8, y: u8) -> i32 {
        self.ore[tile_index(x, y)]
    }

    /// Remove up to `amount` ore from a tile, returning how much was removed.
    pub fn deplete_ore(&mut self, x: u8, y: u8, amount: i32) -> i32 {
        let idx = tile_index(x, y);
        let taken = amount.min(self.ore[idx]);
        self.ore[idx] -= taken;
        taken
    }

    /// Deterministic A* over the static passable grid (8-dir, no corner
    /// cutting). Units do not block movement in v1.
    pub fn find_path(&self, from: (u8, u8), to: (u8, u8)) -> Option<Vec<(u8, u8)>> {
        if from == to {
            return Some(vec![]);
        }
        if !self.is_passable(from.0, from.1) || !self.is_passable(to.0, to.1) {
            return None;
        }

        let start = tile_index(from.0, from.1);
        let goal = tile_index(to.0, to.1);

        let mut g_score = vec![i32::MAX; MAP_TILES];
        g_score[start] = 0;

        let mut came_from = vec![u16::MAX; MAP_TILES];

        #[derive(Clone, Copy, Eq, PartialEq)]
        struct Node {
            f: i32,
            g: i32,
            idx: u16,
        }
        // Min-heap: reverse comparison so BinaryHeap pops the smallest f/g/idx.
        impl Ord for Node {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                other
                    .f
                    .cmp(&self.f)
                    .then_with(|| other.g.cmp(&self.g))
                    .then_with(|| other.idx.cmp(&self.idx))
            }
        }
        impl PartialOrd for Node {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        let mut open = BinaryHeap::new();
        open.push(Node {
            f: octile(from, to),
            g: 0,
            idx: start as u16,
        });

        while let Some(node) = open.pop() {
            let cur = node.idx as usize;
            if cur == goal {
                return Some(reconstruct_path(&came_from, start, goal));
            }
            if node.g > g_score[cur] {
                continue;
            }
            let (cx, cy) = tile_coords(cur);

            for (nx, ny, step) in self.neighbors(cx, cy) {
                let nidx = tile_index(nx, ny);
                let tentative = g_score[cur] + step;
                if tentative < g_score[nidx] {
                    g_score[nidx] = tentative;
                    came_from[nidx] = cur as u16;
                    let f = tentative + octile((nx, ny), to);
                    open.push(Node {
                        f,
                        g: tentative,
                        idx: nidx as u16,
                    });
                }
            }
        }

        None
    }

    /// Passable 8-neighbors with step costs (10 straight, 14 diagonal),
    /// forbidding diagonal corner cutting.
    pub fn neighbors(&self, x: u8, y: u8) -> Vec<(u8, u8, i32)> {
        let mut out = Vec::with_capacity(8);
        for (dx, dy) in &[
            (1i8, 0i8),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ] {
            let nx = x as i32 + *dx as i32;
            let ny = y as i32 + *dy as i32;
            if nx < 0 || ny < 0 || nx >= MAP_SIZE as i32 || ny >= MAP_SIZE as i32 {
                continue;
            }
            let (nx, ny) = (nx as u8, ny as u8);
            if !self.is_passable(nx, ny) {
                continue;
            }
            if *dx != 0 && *dy != 0 {
                // No corner cutting: both orthogonal tiles must be passable.
                if !self.is_passable((x as i32 + *dx as i32) as u8, y)
                    || !self.is_passable(x, (y as i32 + *dy as i32) as u8)
                {
                    continue;
                }
                out.push((nx, ny, 14));
            } else {
                out.push((nx, ny, 10));
            }
        }
        out
    }
}

fn octile(from: (u8, u8), to: (u8, u8)) -> i32 {
    let dx = (from.0 as i32 - to.0 as i32).abs();
    let dy = (from.1 as i32 - to.1 as i32).abs();
    let (dmax, dmin) = if dx > dy { (dx, dy) } else { (dy, dx) };
    10 * dmax + 4 * dmin
}

fn reconstruct_path(came_from: &[u16], start: usize, goal: usize) -> Vec<(u8, u8)> {
    let mut path = vec![];
    let mut cur = goal;
    while cur != start {
        path.push(tile_coords(cur));
        cur = came_from[cur] as usize;
    }
    path.reverse();
    path
}

/// Attempt a single deterministic generation. Returns `None` if the result is
/// not fully connected, so the caller can retry with the next derived seed.
fn try_generate(seed: u64) -> Option<Map> {
    let mut rng = Rng::from_seed(seed);
    let mut passable = vec![true; MAP_TILES];
    let mut ore = vec![0i32; MAP_TILES];

    // Spawn positions (mirrored).
    let hx = rng.range(6, 21) as u8;
    let hy = rng.range(6, 21) as u8;
    let hq0 = (hx, hy);
    let hq1 = (mirror(hx), mirror(hy));

    // Obstacles: symmetric blobs.
    let blobs = 2 + rng.below(3) as usize; // 2..=4
    for _ in 0..blobs {
        let cx = rng.range(4, 60) as u8;
        let cy = rng.range(4, 60) as u8;
        let radius = 2 + rng.below(3) as i32; // 2..=4
        for (x, y) in [(cx, cy), (mirror(cx), mirror(cy))] {
            stamp_blob(&mut passable, x, y, radius);
        }
    }

    // Clear around the HQs so spawns and their main fields are open.
    for (x, y) in [hq0, hq1] {
        clear_around(&mut passable, x, y, 3);
    }

    // Main ore fields: a ring of 8 tiles around each HQ.
    for (x, y) in [hq0, hq1] {
        for (dx, dy) in RING_8 {
            let (ox, oy) = (x as i32 + dx, y as i32 + dy);
            if in_bounds(ox, oy) {
                ore[tile_index(ox as u8, oy as u8)] = 400;
                passable[tile_index(ox as u8, oy as u8)] = true;
            }
        }
    }

    // Expansion sites: generate N pairs (site + mirror).
    let sites = 2 + rng.below(3) as usize; // 2..=4
    let mut placed = 0;
    let mut guard = 0;
    while placed < sites && guard < 200 {
        guard += 1;
        let sx = rng.range(10, 54) as u8;
        let sy = rng.range(10, 54) as u8;
        if !valid_site_center(&passable, sx, sy, hq0, hq1) {
            continue;
        }
        stamp_ore_cluster_symmetric(&mut ore, &mut passable, sx, sy);
        placed += 1;
    }

    let map = Map {
        seed,
        passable,
        ore,
        hq_tiles: [hq0, hq1],
    };

    if is_fully_connected(&map) {
        Some(map)
    } else {
        None
    }
}

fn open_map(seed: u64) -> Map {
    let hq0 = (8u8, 8u8);
    let hq1 = (mirror(8), mirror(8));
    let mut ore = vec![0i32; MAP_TILES];
    for (x, y) in [hq0, hq1] {
        for (dx, dy) in RING_8 {
            let (ox, oy) = (x as i32 + dx, y as i32 + dy);
            if in_bounds(ox, oy) {
                ore[tile_index(ox as u8, oy as u8)] = 400;
            }
        }
    }
    Map {
        seed,
        passable: vec![true; MAP_TILES],
        ore,
        hq_tiles: [hq0, hq1],
    }
}

const RING_8: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

fn in_bounds(x: i32, y: i32) -> bool {
    x >= 0 && y >= 0 && x < MAP_SIZE as i32 && y < MAP_SIZE as i32
}

fn stamp_blob(passable: &mut [bool], cx: u8, cy: u8, radius: i32) {
    let r2 = radius * radius;
    for y in (cy as i32 - radius).max(0)..=(cy as i32 + radius).min(MAP_SIZE as i32 - 1) {
        for x in (cx as i32 - radius).max(0)..=(cx as i32 + radius).min(MAP_SIZE as i32 - 1) {
            let dx = x - cx as i32;
            let dy = y - cy as i32;
            if dx * dx + dy * dy <= r2 {
                passable[tile_index(x as u8, y as u8)] = false;
            }
        }
    }
}

fn clear_around(passable: &mut [bool], cx: u8, cy: u8, radius: i32) {
    for y in (cy as i32 - radius).max(0)..=(cy as i32 + radius).min(MAP_SIZE as i32 - 1) {
        for x in (cx as i32 - radius).max(0)..=(cx as i32 + radius).min(MAP_SIZE as i32 - 1) {
            passable[tile_index(x as u8, y as u8)] = true;
        }
    }
}

/// Stamp a small ore cluster at `(cx, cy)` *and* its point-mirror image, so
/// the map stays exactly symmetric.
fn stamp_ore_cluster_symmetric(ore: &mut [i32], passable: &mut [bool], cx: u8, cy: u8) {
    for (dx, dy) in &[(0i32, 0i32), (1, 0), (0, 1), (1, 1), (-1, 1), (0, -1)] {
        let (x, y) = (cx as i32 + dx, cy as i32 + dy);
        if in_bounds(x, y) {
            let (x, y) = (x as u8, y as u8);
            ore[tile_index(x, y)] = 500;
            passable[tile_index(x, y)] = true;
            let (mx, my) = (mirror(x), mirror(y));
            ore[tile_index(mx, my)] = 500;
            passable[tile_index(mx, my)] = true;
        }
    }
}

fn valid_site_center(passable: &[bool], x: u8, y: u8, hq0: (u8, u8), hq1: (u8, u8)) -> bool {
    if !passable[tile_index(x, y)] {
        return false;
    }
    // Not too close to either HQ (avoid merging main fields).
    let d0 = (x as i32 - hq0.0 as i32)
        .abs()
        .max((y as i32 - hq0.1 as i32).abs());
    let d1 = (x as i32 - hq1.0 as i32)
        .abs()
        .max((y as i32 - hq1.1 as i32).abs());
    d0 > 12 && d1 > 12
}

/// BFS connectivity: from each HQ, every ore tile and the enemy HQ must be
/// reachable over passable tiles (8-dir).
fn is_fully_connected(map: &Map) -> bool {
    for (start, other_hq) in [
        (map.hq_tiles[0], map.hq_tiles[1]),
        (map.hq_tiles[1], map.hq_tiles[0]),
    ] {
        let mut visited = vec![false; MAP_TILES];
        let mut stack = vec![tile_index(start.0, start.1)];
        visited[tile_index(start.0, start.1)] = true;
        while let Some(idx) = stack.pop() {
            let (x, y) = tile_coords(idx);
            for (nx, ny, _) in map.neighbors(x, y) {
                let nidx = tile_index(nx, ny);
                if !visited[nidx] {
                    visited[nidx] = true;
                    stack.push(nidx);
                }
            }
        }
        // Enemy HQ reachable.
        if !visited[tile_index(other_hq.0, other_hq.1)] {
            return false;
        }
        // Every ore tile reachable.
        for (idx, &ore) in map.ore.iter().enumerate() {
            if ore > 0 && !visited[idx] {
                return false;
            }
        }
    }
    true
}

/// A map with no obstacles for scenario tests that need full freedom.
#[allow(dead_code)]
pub fn open_test_map(seed: u64) -> Map {
    open_map(seed)
}

/// Fix-unit position helpers for tile centers (used by spawn logic).
#[allow(dead_code)]
pub fn hq_pos(map: &Map, player: crate::entity::Player) -> Pos {
    let t = map.hq_tiles[player.index()];
    Pos::new(
        t.0 as i32 * FIX_SCALE + FIX_SCALE / 2,
        t.1 as i32 * FIX_SCALE + FIX_SCALE / 2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_is_point_symmetric() {
        for seed in 0..500u64 {
            let map = Map::generate(seed);
            for idx in 0..MAP_TILES {
                let (x, y) = tile_coords(idx);
                let m = tile_index(mirror(x), mirror(y));
                assert_eq!(
                    map.passable[idx], map.passable[m],
                    "passable asymmetry seed {seed}"
                );
                assert_eq!(map.ore[idx], map.ore[m], "ore asymmetry seed {seed}");
            }
            assert_eq!(
                map.hq_tiles[0],
                (mirror(map.hq_tiles[1].0), mirror(map.hq_tiles[1].1))
            );
        }
    }

    #[test]
    fn path_between_hqs_exists() {
        for seed in 0..200u64 {
            let map = Map::generate(seed);
            let p = map.find_path(map.hq_tiles[0], map.hq_tiles[1]);
            assert!(p.is_some(), "no path between HQs for seed {seed}");
        }
    }

    #[test]
    fn pathfinding_is_deterministic() {
        let map = Map::generate(12345);
        let a = map.find_path(map.hq_tiles[0], map.hq_tiles[1]);
        let b = map.find_path(map.hq_tiles[0], map.hq_tiles[1]);
        assert_eq!(a, b);
        // A* on a connected map never returns a path that re-enters start.
        let path = a.unwrap();
        assert!(path.len() < MAP_TILES);
    }
}
