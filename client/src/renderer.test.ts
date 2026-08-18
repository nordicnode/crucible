// Camera math: zoom bounds and map-bounds clamping. Pure logic, no DOM.

import { describe, expect, it } from "vitest";
import { Camera } from "./renderer";

describe("Camera", () => {
  it("clamps zoom to the min/max bounds", () => {
    const c = new Camera();
    c.zoomAt(0, 0, 1000);
    expect(c.zoom).toBeLessThanOrEqual(96);
    c.zoomAt(0, 0, 0.00001);
    expect(c.zoom).toBeGreaterThanOrEqual(4);
  });

  it("centers the viewport on the requested world point", () => {
    const c = new Camera();
    c.focusOn(15.5, 13.5, 18, 800, 600);
    expect(c.screenX(15.5)).toBeCloseTo(400, 5);
    expect(c.screenY(13.5)).toBeCloseTo(300, 5);
  });

  it("keeps the view inside the map when zoomed in", () => {
    const c = new Camera();
    c.setViewport(800, 600);
    c.zoom = 32; // half-view = 12.5 x 9.375 tiles, smaller than the map
    c.cx = -500;
    c.cy = 999;
    c.pan(0, 0); // triggers clamp
    expect(c.cx).toBeGreaterThanOrEqual(12.5);
    expect(c.cx).toBeLessThanOrEqual(64 - 12.5);
    expect(c.cy).toBeGreaterThanOrEqual(9.375);
    expect(c.cy).toBeLessThanOrEqual(64 - 9.375);
  });

  it("centers the map when the viewport is larger than the map", () => {
    const c = new Camera();
    c.setViewport(2000, 2000);
    c.zoom = 4; // half-view is 250 tiles > 64
    c.cx = 3;
    c.cy = 3;
    c.pan(0, 0);
    expect(c.cx).toBe(32);
    expect(c.cy).toBe(32);
  });

  it("zooming in at the screen center keeps the cursor's world point fixed", () => {
    const c = new Camera();
    c.focusOn(30, 30, 18, 800, 600);
    const sx = 200;
    const sy = 150;
    const wx = c.worldX(sx);
    const wy = c.worldY(sy);
    c.zoomAt(sx, sy, 2);
    expect(c.worldX(sx)).toBeCloseTo(wx, 5);
    expect(c.worldY(sy)).toBeCloseTo(wy, 5);
  });
});
