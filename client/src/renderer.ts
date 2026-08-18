// Flat-color Canvas 2D renderer: terrain, fog-of-war, entities, selection,
// HP bars, and a minimap. All positions are world tile coordinates.

import type { Entity } from "./world";
import { World } from "./world";

const MAP = 64;

export class Camera {
  cx = 32;
  cy = 32;
  zoom = 12; // pixels per tile

  screenX(wx: number): number {
    return (wx - this.cx) * this.zoom;
  }
  screenY(wy: number): number {
    return (wy - this.cy) * this.zoom;
  }
  worldX(sx: number): number {
    return sx / this.zoom + this.cx;
  }
  worldY(sy: number): number {
    return sy / this.zoom + this.cy;
  }
  pan(dx: number, dy: number): void {
    this.cx -= dx / this.zoom;
    this.cy -= dy / this.zoom;
  }
  zoomAt(sx: number, sy: number, factor: number): void {
    const wx = this.worldX(sx);
    const wy = this.worldY(sy);
    this.zoom = Math.min(32, Math.max(4, this.zoom * factor));
    this.cx = wx - sx / this.zoom;
    this.cy = wy - sy / this.zoom;
  }
}

const COLORS = {
  unexplored: "#04060a",
  passable: "#17301c",
  impassable: "#2c2f33",
  ore: "#c8920e",
  own: "#4da3ff",
  enemy: "#ff5a5a",
  selected: "#ffe27a",
};

export class Renderer {
  camera = new Camera();

  draw(ctx: CanvasRenderingContext2D, world: World, selection: Set<number>, w: number, h: number): void {
    ctx.fillStyle = COLORS.unexplored;
    ctx.fillRect(0, 0, w, h);

    const cam = this.camera;
    const x0 = Math.max(0, Math.floor(cam.worldX(0)));
    const y0 = Math.max(0, Math.floor(cam.worldY(0)));
    const x1 = Math.min(MAP - 1, Math.ceil(cam.worldX(w)));
    const y1 = Math.min(MAP - 1, Math.ceil(cam.worldY(h)));

    // Terrain + fog.
    for (let ty = y0; ty <= y1; ty++) {
      for (let tx = x0; tx <= x1; tx++) {
        const idx = ty * MAP + tx;
        const px = cam.screenX(tx);
        const py = cam.screenY(ty);
        const size = cam.zoom + 0.5;
        if (world.visible.has(idx)) {
          ctx.fillStyle = world.passable[idx] ? COLORS.passable : COLORS.impassable;
        } else if (world.explored.has(idx)) {
          ctx.fillStyle = world.passable[idx] ? "#0c1a10" : "#17191c";
        } else {
          continue; // already filled dark
        }
        ctx.fillRect(px, py, size, size);
      }
    }

    // Known ore fields.
    for (const t of world.oreTiles.values()) {
      const px = cam.screenX(t.x);
      const py = cam.screenY(t.y);
      const size = cam.zoom + 0.5;
      if (px > w || py > h || px + size < 0 || py + size < 0) continue;
      ctx.fillStyle = COLORS.ore;
      ctx.fillRect(px + size * 0.3, py + size * 0.3, size * 0.4, size * 0.4);
    }

    // Entities: buildings first, then units.
    const drawList = [...world.entities.values()].sort((a, b) => {
      const aUnit = !a.kind.startsWith("Hq") && isUnit(a);
      const bUnit = !b.kind.startsWith("Hq") && isUnit(b);
      if (aUnit !== bUnit) return aUnit ? 1 : -1;
      return a.id - b.id;
    });
    for (const e of drawList) {
      this.drawEntity(ctx, world, e, selection, w, h);
    }

    this.drawMinimap(ctx, world, selection, w, h);
  }

  private drawEntity(
    ctx: CanvasRenderingContext2D,
    world: World,
    e: Entity,
    selection: Set<number>,
    w: number,
    h: number,
  ): void {
    const cam = this.camera;
    const px = cam.screenX(e.x);
    const py = cam.screenY(e.y);
    const z = cam.zoom;
    if (px < -z || py < -z || px > w + z || py > h + z) return;

    // Fade remembered (stale) enemies.
    let alpha = 1;
    if (e.owner === 1 && e.stale != null) {
      const age = Math.max(0, world.tick - e.stale);
      alpha = Math.max(0.15, 1 - age / 600);
    }

    ctx.save();
    ctx.globalAlpha = alpha;
    const color = e.owner === 0 ? COLORS.own : COLORS.enemy;

    if (isUnit(e)) {
      this.drawUnitShape(ctx, e, px, py, z, color);
    } else {
      this.drawBuildingShape(ctx, e, px, py, z, color);
    }

    // Selection ring.
    if (e.owner === 0 && selection.has(e.id)) {
      ctx.strokeStyle = COLORS.selected;
      ctx.lineWidth = 2;
      ctx.strokeRect(px - z * 0.5, py - z * 0.5, z, z);
    }

    // HP bar for own entities with hp info.
    if (e.owner === 0 && e.maxHp > 0) {
      const frac = Math.max(0, e.hp / e.maxHp);
      ctx.fillStyle = "#0a0a0a";
      ctx.fillRect(px - z * 0.4, py - z * 0.7, z * 0.8, 3);
      ctx.fillStyle = frac > 0.5 ? "#4ade80" : frac > 0.25 ? "#facc15" : "#ef4444";
      ctx.fillRect(px - z * 0.4, py - z * 0.7, z * 0.8 * frac, 3);
    }
    ctx.restore();
  }

  private drawUnitShape(
    ctx: CanvasRenderingContext2D,
    e: Entity,
    px: number,
    py: number,
    z: number,
    color: string,
  ): void {
    ctx.fillStyle = color;
    ctx.beginPath();
    switch (e.kind) {
      case "Infantry":
        ctx.arc(px, py, z * 0.28, 0, Math.PI * 2);
        break;
      case "Tank":
        ctx.moveTo(px, py - z * 0.3);
        ctx.lineTo(px - z * 0.3, py + z * 0.3);
        ctx.lineTo(px + z * 0.3, py + z * 0.3);
        break;
      case "Artillery":
        ctx.moveTo(px, py - z * 0.35);
        ctx.lineTo(px + z * 0.3, py);
        ctx.lineTo(px, py + z * 0.35);
        ctx.lineTo(px - z * 0.3, py);
        break;
      case "Harvester":
        ctx.rect(px - z * 0.25, py - z * 0.25, z * 0.5, z * 0.5);
        break;
    }
    ctx.closePath();
    ctx.fill();
  }

  private drawBuildingShape(
    ctx: CanvasRenderingContext2D,
    e: Entity,
    px: number,
    py: number,
    z: number,
    color: string,
  ): void {
    ctx.fillStyle = color;
    const s = e.kind === "Hq" ? 0.5 : 0.4;
    if (e.kind === "Turret") {
      ctx.beginPath();
      ctx.arc(px, py, z * 0.35, 0, Math.PI * 2);
      ctx.fill();
    } else {
      ctx.fillRect(px - z * s, py - z * s, z * s * 2, z * s * 2);
      if (e.kind === "Hq") {
        ctx.fillStyle = "#0a0a0a";
        ctx.fillRect(px - z * 0.2, py - z * 0.2, z * 0.4, z * 0.4);
      }
    }
  }

  private drawMinimap(
    ctx: CanvasRenderingContext2D,
    world: World,
    selection: Set<number>,
    w: number,
    h: number,
  ): void {
    const s = 4; // px per tile
    const ox = w - MAP * s - 8;
    const oy = h - MAP * s - 8;
    ctx.fillStyle = "rgba(0,0,0,0.65)";
    ctx.fillRect(ox - 2, oy - 2, MAP * s + 4, MAP * s + 4);

    for (let ty = 0; ty < MAP; ty++) {
      for (let tx = 0; tx < MAP; tx++) {
        const idx = ty * MAP + tx;
        if (world.visible.has(idx)) {
          ctx.fillStyle = "#3b5b44";
        } else if (world.explored.has(idx)) {
          ctx.fillStyle = "#141a16";
        } else {
          continue;
        }
        ctx.fillRect(ox + tx * s, oy + ty * s, s, s);
      }
    }
    for (const e of world.entities.values()) {
      ctx.fillStyle = e.owner === 0 ? COLORS.own : COLORS.enemy;
      ctx.fillRect(ox + e.x * s - 1, oy + e.y * s - 1, 2, 2);
    }
    if (selection.size > 0) {
      for (const id of selection) {
        const e = world.entities.get(id);
        if (e) {
          ctx.fillStyle = COLORS.selected;
          ctx.fillRect(ox + e.x * s - 1, oy + e.y * s - 1, 2, 2);
        }
      }
    }
  }
}

function isUnit(e: Entity): boolean {
  return ["Harvester", "Infantry", "Tank", "Artillery"].includes(e.kind);
}
