//! Fixed-point integer math.
//!
//! All game-state positions and quantities are integers to guarantee
//! byte-identical behavior across native and wasm builds. One tile is
//! [`FIX_SCALE`] fixed-point units. Transcendental functions and platform
//! float ops are never used in game-state math.

use serde::{Deserialize, Serialize};

/// Fixed point scale: 1 tile == 256 fix units.
pub const FIX_SHIFT: u32 = 8;
pub const FIX_SCALE: i32 = 1 << FIX_SHIFT;
/// Offset from tile origin to tile center.
pub const FIX_HALF: i32 = FIX_SCALE / 2;

/// A fixed-point coordinate (micro-tiles).
pub type Fix = i32;

/// Sim ticks per second of game time. The fixed timestep.
pub const TICKS_PER_SEC: i32 = 10;
/// Command tick: the AI bot cadence issues commands every 2s. Human commands
/// are *not* gated on this — the server applies them on arrival (see
/// crucible-server's ws loop).
pub const COMMAND_TICK: i32 = 20;
/// Match timeout in ticks (15 minutes of game time).
pub const MATCH_TIMEOUT_TICKS: i32 = TICKS_PER_SEC * 60 * 15;

/// Convert a tile coordinate to the fixed-point coordinate of its center.
#[inline]
pub fn tile_center(t: u8) -> Fix {
    (t as i32) * FIX_SCALE + FIX_HALF
}

/// Convert a fixed-point coordinate to the tile it lies in.
#[inline]
pub fn fix_to_tile(f: Fix) -> u8 {
    let t = f / FIX_SCALE;
    t.clamp(0, 63) as u8
}

/// Squared distance between two fixed-point points (i64 to avoid overflow).
#[inline]
pub fn dist2(ax: Fix, ay: Fix, bx: Fix, by: Fix) -> i64 {
    let dx = ax as i64 - bx as i64;
    let dy = ay as i64 - by as i64;
    dx * dx + dy * dy
}

/// Deterministic integer square root (Newton's method, exact for non-negative input).
///
/// Pure integer arithmetic, so identical on every target.
pub fn isqrt(n: i64) -> i64 {
    if n <= 1 {
        return if n < 0 { 0 } else { n };
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Fixed-point coordinate, serialized as a plain struct for snapshots.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug, Default)]
pub struct Pos {
    pub x: Fix,
    pub y: Fix,
}

impl Pos {
    pub const fn new(x: Fix, y: Fix) -> Self {
        Pos { x, y }
    }

    pub const fn from_tile(tx: u8, ty: u8) -> Self {
        Pos {
            x: (tx as i32) * FIX_SCALE + FIX_HALF,
            y: (ty as i32) * FIX_SCALE + FIX_HALF,
        }
    }

    #[inline]
    pub fn tile(&self) -> (u8, u8) {
        (fix_to_tile(self.x), fix_to_tile(self.y))
    }

    #[inline]
    pub fn dist2(&self, other: &Pos) -> i64 {
        dist2(self.x, self.y, other.x, other.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isqrt_is_exact_for_small_squares() {
        for i in 0..10_000i64 {
            let s = isqrt(i * i);
            assert_eq!(s, i, "isqrt({}) = {}, want {}", i * i, s, i);
        }
        for i in 0..10_000i64 {
            let s = isqrt(i);
            assert!(
                s * s <= i && (s + 1) * (s + 1) > i,
                "isqrt({}) off: {}",
                i,
                s
            );
        }
    }

    #[test]
    fn tile_center_round_trips() {
        assert_eq!(tile_center(0), 128);
        assert_eq!(tile_center(63), 63 * 256 + 128);
        assert_eq!(fix_to_tile(tile_center(17)), 17);
        assert_eq!(fix_to_tile(0), 0);
        assert_eq!(fix_to_tile(FIX_SCALE * 63), 63);
    }
}
