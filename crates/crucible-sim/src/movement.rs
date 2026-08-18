//! Deterministic unit movement.
//!
//! Long-range navigation uses A* waypoints ([`Map::find_path`]); the actual
//! per-tick stepping is straight-line integer arithmetic with a simple
//! axis-aligned slide against impassable terrain. No floats, no sqrt beyond
//! the integer `isqrt`.

use crate::entity::Unit;
use crate::fixed::{isqrt, Fix, Pos};
use crate::map::Map;

/// Advance `pos` one tick toward `dest` at `speed` fix-units/tick.
///
/// Returns the new position and whether the unit has arrived (reached `dest`
/// within one step). Slides along an axis when the direct step is blocked.
pub fn step_towards(map: &Map, pos: Pos, dest: Pos, speed: Fix) -> (Pos, bool) {
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

    if map.is_passable(crate::fixed::fix_to_tile(nx), crate::fixed::fix_to_tile(ny)) {
        return (Pos::new(nx, ny), false);
    }

    // Slide: try x-only, then y-only (deterministic preference).
    let x_only = Pos::new(nx, pos.y);
    if map.is_passable(
        crate::fixed::fix_to_tile(x_only.x),
        crate::fixed::fix_to_tile(x_only.y),
    ) {
        return (x_only, false);
    }
    let y_only = Pos::new(pos.x, ny);
    if map.is_passable(
        crate::fixed::fix_to_tile(y_only.x),
        crate::fixed::fix_to_tile(y_only.y),
    ) {
        return (y_only, false);
    }

    (pos, false)
}

/// Move a unit toward the next waypoint on its path; returns true when the
/// unit has reached the waypoint tile (caller pops it).
pub fn follow_path(map: &Map, unit: &mut Unit) -> bool {
    let Some(next) = unit.path.first().copied() else {
        return true;
    };
    let dest = Pos::from_tile(next.0, next.1);
    let speed = crate::entity::unit_stats(unit.utype).speed;
    let (new_pos, arrived) = step_towards(map, unit.pos, dest, speed);
    unit.pos = new_pos;
    if arrived || unit.pos.tile() == next {
        unit.path.remove(0);
        true
    } else {
        false
    }
}

/// Move a unit directly toward a fixed point (no pathing), used for combat
/// chase and harvester hauling. Returns true on arrival.
pub fn move_direct(map: &Map, unit: &mut Unit, dest: Pos) -> bool {
    let speed = crate::entity::unit_stats(unit.utype).speed;
    let (new_pos, arrived) = step_towards(map, unit.pos, dest, speed);
    unit.pos = new_pos;
    arrived
}
