// High-detail Canvas 2D tactical renderer for Crucible:
// Procedural vector terrain, animated ore nodes, detailed team-colored buildings,
// rotating directional units, sci-fi selection reticles, and tactical minimap.

import {
  drawBuildingSprite,
  drawHealthBar,
  drawImpassableTile,
  drawOreDeposit,
  drawPassableTile,
  drawSelectionReticle,
  drawUnitSprite,
} from "./sprites";
import type { Entity } from "./world";
import { World } from "./world";

const MAP = 64;
const ZOOM_MIN = 4; // px per tile (whole map visible on a wide screen)
const ZOOM_MAX = 96; // px per tile (close enough to read unit detail)

export class Camera {
  cx = 32;
  cy = 32;
  zoom = 12; // pixels per tile
  viewportW = 0;
  viewportH = 0;

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
  /** Remember the viewport size and re-apply the map bounds. */
  setViewport(vw: number, vh: number): void {
    this.viewportW = vw;
    this.viewportH = vh;
    this.clampToMap();
  }
  /**
   * Keep the visible viewport inside the map. When the viewport is larger
   * than the map (zoomed way out) the map is centered instead.
   */
  clampToMap(): void {
    if (this.viewportW <= 0 || this.viewportH <= 0) return;
    const halfW = this.viewportW / 2 / this.zoom;
    const halfH = this.viewportH / 2 / this.zoom;
    if (halfW * 2 >= MAP) this.cx = MAP / 2;
    else this.cx = Math.min(MAP - halfW, Math.max(halfW, this.cx));
    if (halfH * 2 >= MAP) this.cy = MAP / 2;
    else this.cy = Math.min(MAP - halfH, Math.max(halfH, this.cy));
  }
  /** Center the viewport on world point (wx, wy) at the given zoom. */
  focusOn(wx: number, wy: number, zoom: number, vw: number, vh: number): void {
    this.zoom = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, zoom));
    this.viewportW = vw;
    this.viewportH = vh;
    // Deliberately unclamped: match start centers on the HQ even when it sits
    // near a map edge. Pan/zoom apply the bounds from there.
    this.cx = wx - vw / 2 / this.zoom;
    this.cy = wy - vh / 2 / this.zoom;
  }
  pan(dx: number, dy: number): void {
    this.cx -= dx / this.zoom;
    this.cy -= dy / this.zoom;
    this.clampToMap();
  }
  zoomAt(sx: number, sy: number, factor: number): void {
    const wx = this.worldX(sx);
    const wy = this.worldY(sy);
    this.zoom = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, this.zoom * factor));
    this.cx = wx - sx / this.zoom;
    this.cy = wy - sy / this.zoom;
    this.clampToMap();
  }
}

const COLORS = {
  unexplored: "#04060a",
  passable: "#16281c",
  impassable: "#22272e",
  ore: "#eab308",
  own: "#2563eb",
  enemy: "#dc2626",
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

    // 1. Terrain + Fog of War
    for (let ty = y0; ty <= y1; ty++) {
      for (let tx = x0; tx <= x1; tx++) {
        const idx = ty * MAP + tx;
        const isVis = world.visible.has(idx);
        const isExp = world.explored.has(idx);
        if (!isVis && !isExp) continue; // Unexplored void

        const px = cam.screenX(tx);
        const py = cam.screenY(ty);
        const size = cam.zoom + 0.5;
        const isPassable = world.passable[idx] ?? true;

        if (isPassable) {
          drawPassableTile(ctx, tx, ty, px, py, size, !isVis);
        } else {
          drawImpassableTile(ctx, tx, ty, px, py, size, !isVis);
        }
      }
    }

    // 2. Ore Fields (Luminous Crystal Deposits)
    for (const t of world.oreTiles.values()) {
      const px = cam.screenX(t.x);
      const py = cam.screenY(t.y);
      const size = cam.zoom;
      if (px > w || py > h || px + size < 0 || py + size < 0) continue;
      drawOreDeposit(ctx, px, py, size, t.amount, world.tick);
    }

    // 3. Entities: Buildings first (so units appear on top), then units
    const drawList = [...world.entities.values()].sort((a, b) => {
      const aUnit = isUnit(a);
      const bUnit = isUnit(b);
      if (aUnit !== bUnit) return aUnit ? 1 : -1;
      return a.id - b.id;
    });

    for (const e of drawList) {
      this.drawEntity(ctx, world, e, selection, w, h);
    }

    // The radar minimap is drawn separately into the DOM sidebar canvas
    // (`drawRadar`), so it lives in the C&C-style frame instead of floating
    // over the map.
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
    const p = world.pos(e.id);
    const px = cam.screenX(p.x);
    const py = cam.screenY(p.y);
    const z = cam.zoom;
    if (px < -z * 2 || py < -z * 2 || px > w + z * 2 || py > h + z * 2) return;

    // Fade remembered (stale) enemy radar blips
    let isStale = false;
    let alpha = 1;
    if (e.owner === 1 && e.stale != null) {
      isStale = true;
      const age = Math.max(0, world.tick - e.stale);
      alpha = Math.max(0.2, 1 - age / 600);
    }

    ctx.save();
    ctx.globalAlpha = alpha;

    const isSelected = e.owner === 0 && selection.has(e.id);

    if (isUnit(e)) {
      const heading = world.heading(e.id);
      drawUnitSprite(ctx, e.kind, px, py, z, e.owner, heading, world.tick, isStale, 0);
    } else {
      drawBuildingSprite(
        ctx,
        e.kind,
        px,
        py,
        z,
        e.owner,
        world.tick,
        isStale,
        e.progress ?? 0,
        e.buildTime ?? 0,
      );
    }

    // Sci-fi 4-corner Selection Reticle
    if (isSelected) {
      const reticleSize = isUnit(e) ? z * 0.9 : z * 1.15;
      drawSelectionReticle(ctx, px, py, reticleSize, world.tick);
    }

    // HP Bar for own entities with HP
    if (e.owner === 0 && e.maxHp > 0) {
      const barSize = isUnit(e) ? z * 0.8 : z * 1.1;
      drawHealthBar(ctx, px, py, barSize, e.hp, e.maxHp);
    }

    ctx.restore();
  }

}

/**
 * Draw the tactical radar into a dedicated DOM canvas (the C&C-style frame in
 * the sidebar). `vw`/`vh` are the main viewport dimensions used to compute the
 * camera viewport rectangle.
 */
export function drawRadar(
  ctx: CanvasRenderingContext2D,
  world: World,
  cam: Camera,
  selection: Set<number>,
  vw: number,
  vh: number,
): void {
  const W = ctx.canvas.width;
  const H = ctx.canvas.height;
  const s = Math.min(W / MAP, H / MAP);
  const ox = (W - MAP * s) / 2;
  const oy = (H - MAP * s) / 2;

  // Steel-dark radar screen: unexplored reads as a panel, not black void.
  ctx.fillStyle = "#131b24";
  ctx.fillRect(0, 0, W, H);
  ctx.fillStyle = "#0e141c";
  ctx.fillRect(ox, oy, MAP * s, MAP * s);

  for (let ty = 0; ty < MAP; ty++) {
    for (let tx = 0; tx < MAP; tx++) {
      const idx = ty * MAP + tx;
      if (world.visible.has(idx)) {
        ctx.fillStyle = world.passable[idx] ? "#284d33" : "#3a4654";
      } else if (world.explored.has(idx)) {
        ctx.fillStyle = world.passable[idx] ? "#16271c" : "#1c242e";
      } else {
        ctx.fillStyle = "#101720";
      }
      ctx.fillRect(ox + tx * s, oy + ty * s, Math.ceil(s), Math.ceil(s));
    }
  }

  // Ore fields: bright gold dots.
  for (const t of world.oreTiles.values()) {
    if (t.amount > 0) {
      ctx.fillStyle = "#ffd75e";
      ctx.fillRect(ox + t.x * s, oy + t.y * s, Math.max(1, Math.ceil(s)), Math.max(1, Math.ceil(s)));
    }
  }

  // Entities: own = blue, enemy = red, selected = gold.
  for (const e of world.entities.values()) {
    const p = world.pos(e.id);
    const isSel = selection.has(e.id);
    ctx.fillStyle = isSel ? COLORS.selected : e.owner === 0 ? COLORS.own : COLORS.enemy;
    const dot = isUnit(e) ? 2 : 3;
    ctx.fillRect(ox + Math.floor(p.x * s) - 1, oy + Math.floor(p.y * s) - 1, dot, dot);
  }

  // Camera viewport rectangle.
  const px = (cam.worldX(0) / MAP) * (MAP * s);
  const py = (cam.worldY(0) / MAP) * (MAP * s);
  const pw = Math.min(MAP * s, (vw / cam.zoom) * s);
  const ph = Math.min(MAP * s, (vh / cam.zoom) * s);
  ctx.strokeStyle = "rgba(255, 255, 255, 0.55)";
  ctx.lineWidth = 1;
  ctx.strokeRect(ox + Math.max(0, px), oy + Math.max(0, py), Math.max(2, pw), Math.max(2, ph));

  // Scanline sheen for the radar feel.
  ctx.fillStyle = "rgba(255, 255, 255, 0.02)";
  for (let y = 0; y < H; y += 3) ctx.fillRect(0, y, W, 1);
}

function isUnit(e: Entity): boolean {
  return ["Harvester", "Infantry", "Tank", "Artillery"].includes(e.kind);
}

