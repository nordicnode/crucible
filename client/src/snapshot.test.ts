import { describe, expect, it } from "vitest";
import { applyFrame, applyMeta, type ReplayFrame, type ReplayMeta } from "./snapshot";
import { World } from "./world";

function frameFixture(units: ReplayFrame["units"], buildings: ReplayFrame["buildings"] = []): ReplayFrame {
  return { tick: 10, ore0: 0, ore1: 0, units, buildings, winner: null, win_reason: null };
}

function metaFixture(): ReplayMeta {
  const passable = new Array<boolean>(64 * 64).fill(true);
  const ore = new Array<number>(64 * 64).fill(0);
  ore[10 * 64 + 10] = 400;
  return {
    map_seed: 7,
    passable,
    hq_tiles: [
      [8, 8],
      [55, 55],
    ],
    ore,
    duration_ticks: 1000,
    winner: null,
    win_reason: null,
  };
}

describe("snapshot mapping", () => {
  it("applies map meta with full visibility and ore tiles", () => {
    const w = new World();
    applyMeta(w, metaFixture());
    expect(w.mapSeed).toBe(7);
    expect(w.hq).toEqual([
      [8, 8],
      [55, 55],
    ]);
    expect(w.visible.size).toBe(64 * 64);
    expect(w.oreTiles.get("10,10")).toEqual({ x: 10, y: 10, amount: 400 });
  });

  it("maps a frame's units/buildings into entities (camelCase maxHp)", () => {
    const w = new World();
    applyMeta(w, metaFixture());
    const frame: ReplayFrame = {
      tick: 50,
      ore0: 300,
      ore1: 400,
      units: [{ id: 3, kind: "Infantry", owner: 0, x: 8.5, y: 9.5, hp: 40, max_hp: 40 }],
      buildings: [{ id: 1, kind: "Hq", owner: 0, x: 8.5, y: 8.5, hp: 1500, max_hp: 1500 }],
      winner: null,
      win_reason: null,
    };
    applyFrame(w, frame);
    expect(w.tick).toBe(50);
    expect(w.ore).toBe(300);
    expect(w.entities.get(3)).toMatchObject({ kind: "Infantry", owner: 0, maxHp: 40 });
    expect(w.entities.get(1)?.kind).toBe("Hq");
  });

  it("sets the result when the frame has a winner", () => {
    const w = new World();
    applyMeta(w, metaFixture());
    applyFrame(w, {
      tick: 100,
      ore0: 0,
      ore1: 500,
      units: [],
      buildings: [],
      winner: 1,
      win_reason: "HqDestroyed",
    });
    expect(w.result).toEqual({ winner: 1, reason: "HqDestroyed" });
  });
});

describe("movement interpolation", () => {
  it("interpolates display positions toward the authoritative target", () => {
    const w = new World();
    w.setMap(7, metaFixture().passable, [[8, 8], [55, 55]]);
    const unit = (x: number, y: number) => [
      { id: 3, kind: "Infantry", owner: 0, x, y, hp: 40, maxHp: 40 },
    ];
    w.applyDiff(10, 0, unit(10, 10), [], [], []);
    // New entity snaps to its first reported position.
    expect(w.pos(3)).toEqual({ x: 10, y: 10 });

    // Target moves to 12,12; display follows gradually (50ms → half way).
    w.applyDiff(11, 0, unit(12, 12), [], [], []);
    w.advance(50);
    const p = w.pos(3);
    expect(p.x).toBeGreaterThan(10);
    expect(p.x).toBeLessThan(12);
    expect(p.x).toBeCloseTo(11, 5);
    expect(p.y).toBeCloseTo(11, 5);
  });

  it("drops display state for removed entities", () => {
    const w = new World();
    w.setMap(7, metaFixture().passable, [[8, 8], [55, 55]]);
    const unit = (x: number, y: number) => [
      { id: 3, kind: "Infantry", owner: 0, x, y, hp: 40, maxHp: 40 },
    ];
    w.applyDiff(10, 0, unit(1, 1), [], [], []);
    w.applyDiff(11, 0, [], [], [], []);
    expect(w.entities.has(3)).toBe(false);
    expect(w.display.has(3)).toBe(false);
  });

  it("applyFrame carries display positions across frames and prunes deaths", () => {
    const w = new World();
    applyMeta(w, metaFixture());
    const unit = (x: number, y: number) => [
      { id: 3, kind: "Infantry", owner: 0, x, y, hp: 40, max_hp: 40 },
    ];

    // New entity snaps to its first reported position.
    applyFrame(w, frameFixture(unit(10, 10)));
    expect(w.pos(3)).toEqual({ x: 10, y: 10 });

    // Next frame moves the target; display starts from the old position so
    // playback can interpolate instead of teleporting.
    applyFrame(w, frameFixture(unit(12, 12)));
    expect(w.entities.get(3)?.x).toBe(12);
    expect(w.pos(3).x).toBeLessThan(12);
    w.advance(100);
    expect(w.pos(3).x).toBeCloseTo(12, 5);

    // A vanished entity leaves no display ghost behind.
    applyFrame(w, frameFixture([]));
    expect(w.entities.has(3)).toBe(false);
    expect(w.display.has(3)).toBe(false);
  });

  it("applyMeta clears render state from a previously loaded session", () => {
    const w = new World();
    w.applyDiff(
      5,
      0,
      [{ id: 9, kind: "Tank", owner: 0, x: 3, y: 3, hp: 120, maxHp: 120 }],
      [],
      [],
      [],
    );
    expect(w.display.size).toBe(1);
    expect(w.headings.size).toBe(1);

    applyMeta(w, metaFixture());
    expect(w.display.size).toBe(0);
    expect(w.headings.size).toBe(0);
    expect(w.result).toBeNull();
  });
});
