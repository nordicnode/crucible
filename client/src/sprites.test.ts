import { describe, expect, it } from "vitest";
import {
  drawBuildingSprite,
  drawHealthBar,
  drawImpassableTile,
  drawOreDeposit,
  drawPassableTile,
  drawRallyPoint,
  drawSelectionReticle,
  drawUnitSprite,
  getTeamPalette,
  TEAM_BLUE,
  TEAM_RED,
  TEAM_STALE,
} from "./sprites";

class MockCanvasContext {
  fillStyle: string | CanvasGradient | CanvasPattern = "";
  strokeStyle: string | CanvasGradient | CanvasPattern = "";
  lineWidth = 1;
  globalAlpha = 1;
  lineDashOffset = 0;

  save(): void {}
  restore(): void {}
  translate(_x: number, _y: number): void {}
  rotate(_angle: number): void {}
  beginPath(): void {}
  closePath(): void {}
  moveTo(_x: number, _y: number): void {}
  lineTo(_x: number, _y: number): void {}
  stroke(): void {}
  fill(): void {}
  fillRect(_x: number, _y: number, _w: number, _h: number): void {}
  strokeRect(_x: number, _y: number, _w: number, _h: number): void {}
  arc(_x: number, _y: number, _r: number, _s: number, _e: number): void {}
  ellipse(_x: number, _y: number, _rx: number, _ry: number, _rot: number, _s: number, _e: number): void {}
  quadraticCurveTo(_cpx: number, _cpy: number, _x: number, _y: number): void {}
  roundRect(_x: number, _y: number, _w: number, _h: number, _r: number): void {}
  setLineDash(_segments: number[]): void {}
  createRadialGradient(_x0: number, _y0: number, _r0: number, _x1: number, _y1: number, _r1: number): CanvasGradient {
    return { addColorStop: (_offset: number, _color: string) => {} } as unknown as CanvasGradient;
  }
}

describe("Team palettes", () => {
  it("returns blue for P0 and red for P1", () => {
    expect(getTeamPalette(0)).toEqual(TEAM_BLUE);
    expect(getTeamPalette(1)).toEqual(TEAM_RED);
    expect(getTeamPalette(1, true)).toEqual(TEAM_STALE);
  });
});

describe("Terrain sprite rendering", () => {
  const ctx = new MockCanvasContext() as unknown as CanvasRenderingContext2D;

  it("renders passable tiles for active and fogged views", () => {
    expect(() => drawPassableTile(ctx, 5, 10, 50, 100, 18, false)).not.toThrow();
    expect(() => drawPassableTile(ctx, 5, 10, 50, 100, 18, true)).not.toThrow();
  });

  it("renders impassable tiles for active and fogged views", () => {
    expect(() => drawImpassableTile(ctx, 8, 12, 80, 120, 18, false)).not.toThrow();
    expect(() => drawImpassableTile(ctx, 8, 12, 80, 120, 18, true)).not.toThrow();
  });

  it("renders ore deposits at varying depletion amounts", () => {
    expect(() => drawOreDeposit(ctx, 100, 100, 18, 500, 10)).not.toThrow();
    expect(() => drawOreDeposit(ctx, 100, 100, 18, 100, 10)).not.toThrow();
  });
});

describe("Building sprite rendering", () => {
  const ctx = new MockCanvasContext() as unknown as CanvasRenderingContext2D;
  const buildings = ["Hq", "Refinery", "Barracks", "Factory", "TechLab", "Turret"];

  for (const b of buildings) {
    it(`renders ${b} for P0, P1, and stale state`, () => {
      expect(() => drawBuildingSprite(ctx, b, 100, 100, 18, 0, 5)).not.toThrow();
      expect(() => drawBuildingSprite(ctx, b, 100, 100, 18, 1, 5)).not.toThrow();
      expect(() => drawBuildingSprite(ctx, b, 100, 100, 18, 1, 5, true)).not.toThrow();
    });

    it(`renders ${b} with active production progress bar`, () => {
      expect(() => drawBuildingSprite(ctx, b, 100, 100, 18, 0, 5, false, 50, 100)).not.toThrow();
    });
  }
});

describe("Unit sprite rendering", () => {
  const ctx = new MockCanvasContext() as unknown as CanvasRenderingContext2D;
  const units = ["Infantry", "Tank", "Artillery", "Harvester"];

  for (const u of units) {
    it(`renders ${u} with direction and owner`, () => {
      expect(() => drawUnitSprite(ctx, u, 50, 50, 18, 0, Math.PI / 4, 10)).not.toThrow();
      expect(() => drawUnitSprite(ctx, u, 50, 50, 18, 1, -Math.PI / 2, 10)).not.toThrow();
      expect(() => drawUnitSprite(ctx, u, 50, 50, 18, 1, 0, 10, true)).not.toThrow();
    });
  }

  it("renders Harvester with loaded ore cargo", () => {
    expect(() => drawUnitSprite(ctx, "Harvester", 50, 50, 18, 0, 0, 10, false, 50)).not.toThrow();
  });
});

describe("Tactical FX & Reticles", () => {
  const ctx = new MockCanvasContext() as unknown as CanvasRenderingContext2D;

  it("renders selection reticle", () => {
    expect(() => drawSelectionReticle(ctx, 50, 50, 20, 15)).not.toThrow();
  });

  it("renders health bars across various HP percentages", () => {
    expect(() => drawHealthBar(ctx, 50, 50, 20, 100, 100)).not.toThrow();
    expect(() => drawHealthBar(ctx, 50, 50, 20, 40, 100)).not.toThrow();
    expect(() => drawHealthBar(ctx, 50, 50, 20, 10, 100)).not.toThrow();
  });

  it("renders rally waypoint path and beacon", () => {
    expect(() => drawRallyPoint(ctx, 50, 50, 100, 120, 25)).not.toThrow();
  });
});
