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
    this.oreTiles = new Map(oreTiles.map((t) => [`${t.x},${t.y}`, t]));
    this.visible = new Set(visible);
    for (const v of visible) this.explored.add(v);
    if (events.length > 0) {
      this.events.push(...events);
      if (this.events.length > 12) this.events = this.events.slice(-12);
    }
  }
}
