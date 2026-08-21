//! Deterministic unit movement.
//!
//! Long-range navigation uses A* waypoints ([`Map::find_path`]); the actual
//! per-tick stepping is straight-line integer arithmetic with a simple
//! axis-aligned slide against impassable terrain *and* blocked tiles
//! (buildings). No floats, no sqrt beyond the integer `isqrt`.
//!
//! Every primitive takes a `blocked: &[bool]` overlay (4096 entries, one per
//! tile) marking tiles occupied by buildings. Terrain passability comes from
//! [`Map`] itself; the overlay is the dynamic layer on top.

use crate::fixed::{isqrt, Fix, Pos};
use crate::map::{tile_index, Map, MAP_SIZE};

/// Advance `pos` one tick toward `dest` at `speed` fix-units/tick.
///
/// Returns the new position and whether the unit has arrived (reached `dest`
/// within one step). Slides along an axis when the direct step is blocked.
/// `fly` (aircraft) ignores building blockers — it flies over buildings —
/// while still respecting map terrain passability.
pub fn step_towards(
    map: &Map,
    blocked: &[bool],
    pos: Pos,
    dest: Pos,
    speed: Fix,
    fly: bool,
) -> (Pos, bool) {
    let dx = dest.x as i64 - pos.x as i64;
    let dy = dest.y as i64 - pos.y as i64;
    let d2 = dx * dx + dy * dy;
    let dist = isqrt(d2);

    if dist == 0 || dist <= speed as i64 {
        return (dest, true);
    }

    let step_x = (dx * speed as i64 / dist) as Fix;
    let step_y = (dy * speed as i64 / dist) as Fix;

    let nx = pos.x + step_x;
    let ny = pos.y + step_y;

    if tile_free(map, blocked, nx, ny, fly) {
        return (Pos::new(nx, ny), false);
    }

    // Slide: try x-only, then y-only (deterministic preference).
    let x_only = Pos::new(nx, pos.y);
    if tile_free(map, blocked, x_only.x, x_only.y, fly) {
        return (x_only, false);
    }
    let y_only = Pos::new(pos.x, ny);
    if tile_free(map, blocked, y_only.x, y_only.y, fly) {
        return (y_only, false);
    }

    (pos, false)
}

/// Is the fix-position's tile free (passable terrain, and not blocked unless
/// the unit flies over buildings)?
fn tile_free(map: &Map, blocked: &[bool], x: Fix, y: Fix, fly: bool) -> bool {
    let tx = crate::fixed::fix_to_tile(x);
    let ty = crate::fixed::fix_to_tile(y);
    if tx >= MAP_SIZE as u8 || ty >= MAP_SIZE as u8 {
        return false;
    }
    map.is_passable(tx, ty) && (fly || !blocked[tile_index(tx, ty)])
}

/// Destination tile for unit `index` of a group moving to `waypoint`: a small
/// deterministic ring offset so the group arrives spread out instead of
/// stacked on one tile. Falls back to the waypoint itself when the offset
/// tile is invalid (out of bounds / impassable / blocked).
// Tight cluster within one tile: keeps groups cohesive (no trickle-in) while
// still preventing the all-on-one-tile stack.
const FORMATION_OFFSETS: [(i32, i32); 9] = [
    (0, 0),
    (1, 0),
    (0, 1),
    (-1, 0),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

pub fn formation_tile(
    waypoint: (u8, u8),
    index: usize,
    map: &Map,
    blocked: &[bool],
    fly: bool,
) -> (u8, u8) {
    let (dx, dy) = FORMATION_OFFSETS[index % FORMATION_OFFSETS.len()];
    let x = waypoint.0 as i32 + dx;
    let y = waypoint.1 as i32 + dy;
    if x >= 0 && y >= 0 && x < MAP_SIZE as i32 && y < MAP_SIZE as i32 {
        let t = (x as u8, y as u8);
        if map.is_passable(t.0, t.1) && (fly || !blocked[tile_index(t.0, t.1)]) {
            return t;
        }
    }
    waypoint
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::{fix_to_tile, FIX_SCALE};
    use crate::map::MAP_TILES;

    #[test]
    fn flyers_step_over_blocked_tiles_ground_does_not() {
        let map = Map::generate(1);
        let mut blocked = vec![false; MAP_TILES];
        blocked[tile_index(11, 10)] = true; // a building on the straight path

        let pos = Pos::from_tile(10, 10);
        let dest = Pos::from_tile(12, 10);

        // Ground unit: the blocked tile stops it (slides, staying put).
        let (gp, _) = step_towards(&map, &blocked, pos, dest, FIX_SCALE, false);
        assert_eq!(fix_to_tile(gp.x), 10, "ground unit walked into a building");

        // Aircraft: flies straight over the blocked tile.
        let (fp, _) = step_towards(&map, &blocked, pos, dest, FIX_SCALE, true);
        assert_eq!(
            fix_to_tile(fp.x),
            11,
            "aircraft did not fly over the building"
        );
    }

    #[test]
    fn formation_tile_allows_aircraft_over_buildings() {
        let map = Map::generate(1);
        let mut blocked = vec![false; MAP_TILES];
        blocked[tile_index(11, 11)] = true;
        // Ground: the blocked offset is rejected, the waypoint itself is used.
        assert_eq!(formation_tile((10, 10), 0, &map, &blocked, false), (10, 10));
        // Aircraft: the (1,1) offset (index 5) lands on the blocked tile and
        // is accepted for flyers.
        assert_eq!(formation_tile((10, 10), 5, &map, &blocked, true), (11, 11));
    }
}
