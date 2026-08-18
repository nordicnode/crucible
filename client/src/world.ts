// Client-side render state: a fogged view of the match, updated by StateDiff.
// No game rules live here.

import type { DiffEntity, OreTile } from "./types";
import { BUILDING_KINDS, UNIT_KINDS } from "./types";

export interface Entity extends DiffEntity {}

export class World {
  mapSeed = 0;
  passable: boolean[] = [];
  hq: [number, number][] = [];
  tick = 0;
  ore = 0;
  entities = new Map<number, Entity>();
  /** Smoothly-interpolated render positions (targets are `entities`). */
  display = new Map<number, { x: number; y: number }>();
  /** Current visual facing angles (radians) for units. */
  headings = new Map<number, number>();
  oreTiles = new Map<string, OreTile>();
  explored = new Set<number>();
  visible = new Set<number>();
  events: { tick: number; kind: string }[] = [];
  result: { winner: number | null; reason: string | null } | null = null;

  get ownUnits(): Entity[] {
    return [...this.entities.values()].filter((e) => e.owner === 0 && UNIT_KINDS.has(e.kind));
  }
  get ownBuildings(): Entity[] {
    return [...this.entities.values()].filter((e) => e.owner === 0 && BUILDING_KINDS.has(e.kind));
  }
  get enemyEntities(): Entity[] {
    return [...this.entities.values()].filter((e) => e.owner === 1);
  }

  setMap(mapSeed: number, passable: boolean[], hq: [number, number][]): void {
    this.mapSeed = mapSeed;
    this.passable = passable;
    this.hq = hq;
  }

  applyDiff(
    tick: number,
    ore: number,
    entities: DiffEntity[],
    oreTiles: OreTile[],
    visible: number[],
    events: { tick: number; kind: string }[],
  ): void {
    this.tick = tick;
    this.ore = ore;
    this.entities = new Map(entities.map((e) => [e.id, e]));
    // Carry over display positions for entities that still exist; new
    // entities snap to their first reported position.
    for (const e of entities) {
      const cur = this.display.get(e.id);
      this.display.set(e.id, {
        x: cur ? cur.x : e.x,
        y: cur ? cur.y : e.y,
      });
      if (!this.headings.has(e.id)) {
        // Default facing: P0 faces bottom-right (+x, +y), P1 faces top-left (-x, -y)
        const defaultAngle = e.owner === 0 ? Math.PI / 4 : -3 * Math.PI / 4;
        this.headings.set(e.id, defaultAngle);
      }
    }
    for (const id of [...this.display.keys()]) {
      if (!this.entities.has(id)) {
        this.display.delete(id);
        this.headings.delete(id);
      }
    }
    this.oreTiles = new Map(oreTiles.map((t) => [`${t.x},${t.y}`, t]));
    this.visible = new Set(visible);
    for (const v of visible) this.explored.add(v);
    if (events.length > 0) {
      this.events.push(...events);
      if (this.events.length > 12) this.events = this.events.slice(-12);
    }
  }

  /**
   * Move display positions toward their authoritative targets. Call once per
   * rendered frame with the elapsed time since the last frame (ms). A 100ms
   * time constant makes movement smooth between the server's 10Hz state
   * diffs while still snapping new entities into place quickly.
   */
  advance(dtMs: number): void {
    const f = Math.min(1, dtMs / 100);
    for (const e of this.entities.values()) {
      const d = this.display.get(e.id);
      if (!d) continue;
      const dx = e.x - d.x;
      const dy = e.y - d.y;
      if (Math.hypot(dx, dy) > 0.01) {
        const targetHeading = Math.atan2(dy, dx);
        const curHeading = this.headings.get(e.id) ?? targetHeading;
        // Shortest angular difference
        let diff = targetHeading - curHeading;
        while (diff < -Math.PI) diff += Math.PI * 2;
        while (diff > Math.PI) diff -= Math.PI * 2;
        this.headings.set(e.id, curHeading + diff * Math.min(1, f * 1.5));
      }
      d.x += dx * f;
      d.y += dy * f;
    }
  }

  /** Display (interpolated) position of an entity, falling back to its target. */
  pos(id: number): { x: number; y: number } {
    const e = this.entities.get(id);
    const d = this.display.get(id);
    if (d && e) return { x: d.x, y: d.y };
    if (e) return { x: e.x, y: e.y };
    return { x: 0, y: 0 };
  }

  /** Current facing angle in radians of an entity. */
  heading(id: number): number {
    return this.headings.get(id) ?? 0;
  }
}
