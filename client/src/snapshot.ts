// Pure mapping from the wasm replay shim's meta/frame JSON into the client's
// World render state. No game rules here — this only reshapes serialized
// sim state into the shapes the renderer already draws. Kept wasm-free so it
// is unit-testable.

import type { DiffEntity, OreTile } from "./types";
import { World } from "./world";

const MAP = 64;

/** A single entity in a spectate frame (both players, full state, no fog). */
export interface FrameEntity {
  id: number;
  kind: string;
  owner: number;
  x: number;
  y: number;
  hp: number;
  max_hp: number;
}

/** Static metadata for a replay: map + recorded outcome. */
export interface ReplayMeta {
  map_seed: number;
  passable: boolean[];
  hq_tiles: [number, number][];
  ore: number[];
  duration_ticks: number;
  winner: number | null;
  win_reason: string | null;
}

/** One spectate frame at a tick. */
export interface ReplayFrame {
  tick: number;
  ore0: number;
  ore1: number;
  units: FrameEntity[];
  buildings: FrameEntity[];
  winner: number | null;
  win_reason: string | null;
}

/** Seed the world with a replay's map, shown with full visibility. */
export function applyMeta(world: World, meta: ReplayMeta): void {
  world.mapSeed = meta.map_seed;
  world.passable = meta.passable;
  world.hq = meta.hq_tiles;
  world.oreTiles = new Map<string, OreTile>();
  for (let y = 0; y < MAP; y++) {
    for (let x = 0; x < MAP; x++) {
      const amount = meta.ore[y * MAP + x];
      if (amount > 0) world.oreTiles.set(`${x},${y}`, { x, y, amount });
    }
  }
  // Spectate shows the whole map: no fog.
  const all = new Set<number>();
  for (let i = 0; i < MAP * MAP; i++) all.add(i);
  world.visible = all;
  world.explored = new Set(all);
  world.entities = new Map();
  // Drop display positions/headings from any previously loaded replay or
  // match so entities can't ghost-render at stale coordinates.
  world.resetRenderState();
  world.tick = 0;
  world.ore = 0;
  world.events = [];
  world.result = null;
}

/** Replace the world's entities/score with one spectate frame. */
export function applyFrame(world: World, frame: ReplayFrame): void {
  const entities: DiffEntity[] = [
    ...frame.units.map((u) => ({
      id: u.id,
      kind: u.kind,
      owner: u.owner,
      x: u.x,
      y: u.y,
      hp: u.hp,
      maxHp: u.max_hp,
    })),
    ...frame.buildings.map((b) => ({
      id: b.id,
      kind: b.kind,
      owner: b.owner,
      x: b.x,
      y: b.y,
      hp: b.hp,
      maxHp: b.max_hp,
    })),
  ];
  // Reuse the live-diff pipeline so spectate gets the same display-position
  // carry-over, default headings, and stale-entity pruning as a live match.
  // The map/visibility state set by applyMeta is passed through unchanged.
  world.applyDiff(
    frame.tick,
    frame.ore0,
    entities,
    [...world.oreTiles.values()],
    [...world.visible],
    [],
  );
  world.result =
    frame.winner == null
      ? null
      : { winner: frame.winner, reason: frame.win_reason };
}
