import { describe, expect, it } from "vitest";
import { applyFrame, applyMeta, type ReplayFrame, type ReplayMeta } from "./snapshot";
import { World } from "./world";

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
