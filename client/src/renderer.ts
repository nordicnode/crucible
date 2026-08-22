// Canvas 2D tactical renderer for Crucible:
// Integer-cell terrain, indexed pixel sprites, directional military hardware,
// blocky combat FX, and a compact tactical minimap.

import { fx } from "./fx";
import { drawHealthBar, drawSelectionReticle } from "./hud";
import {
  drawBuildingSprite,
  drawImpassableTile,
  drawOreDeposit,
  drawPassableTile,
  drawUnitSprite,
} from "./sprites";
import { BUILDING_KINDS, BUILD_COSTS, UNIT_KINDS } from "./types";
import type { Entity } from "./world";
import { World } from "./world";

export const MAP = 64;
export const ZOOM_MIN = 4;
export const ZOOM_MAX = 96;

export class Camera {
  cx = 32; // World coordinate at top-left of viewport
  cy = 32;
  zoom = 18; // Pixels per world tile
  viewportW = 800;
  viewportH = 600;

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

  setViewport(vw: number, vh: number): void {
    this.viewportW = vw;
    this.viewportH = vh;
    this.clampToMap();
  }

  clampToMap(): void {
    if (this.viewportW <= 0 || this.viewportH <= 0) return;
    const viewW = this.viewportW / this.zoom;
    const viewH = this.viewportH / this.zoom;

    if (viewW >= MAP) {
      this.cx = (MAP - viewW) / 2;
    } else {
      this.cx = Math.max(0, Math.min(MAP - viewW, this.cx));
    }

    if (viewH >= MAP) {
      this.cy = (MAP - viewH) / 2;
    } else {
      this.cy = Math.max(0, Math.min(MAP - viewH, this.cy));
    }
  }

  focusOn(wx: number, wy: number, zoom: number, vw: number, vh: number): void {
    this.zoom = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, zoom));
    this.viewportW = vw;
    this.viewportH = vh;
    this.cx = wx - vw / (2 * this.zoom);
    this.cy = wy - vh / (2 * this.zoom);
  }

  centerOn(wx: number, wy: number, vw: number, vh: number, zoom?: number): void {
    this.focusOn(wx, wy, zoom ?? this.zoom, vw, vh);
  }

  pan(dx: number, dy: number, vw?: number, vh?: number): void {
    if (vw != null && vh != null) {
      this.viewportW = vw;
      this.viewportH = vh;
    }
    this.cx -= dx / this.zoom;
    this.cy -= dy / this.zoom;
    this.clampToMap();
  }

  zoomAt(sx: number, sy: number, factor: number, vw?: number, vh?: number): void {
    if (vw != null && vh != null) {
      this.viewportW = vw;
      this.viewportH = vh;
    }
    const wx = this.worldX(sx);
    const wy = this.worldY(sy);
    this.zoom = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, this.zoom * factor));
    this.cx = wx - sx / this.zoom;
    this.cy = wy - sy / this.zoom;
    this.clampToMap();
  }
}

export function cameraViewRect(
  c: Camera,
  w: number,
  h: number,
): { x: number; y: number; w: number; h: number } | null {
  const x0 = c.worldX(0);
  const y0 = c.worldY(0);
  const x1 = c.worldX(w);
  const y1 = c.worldY(h);
  const rx0 = Math.max(0, Math.min(MAP, x0));
  const ry0 = Math.max(0, Math.min(MAP, y0));
  const rx1 = Math.max(0, Math.min(MAP, x1));
  const ry1 = Math.max(0, Math.min(MAP, y1));
  if (rx1 <= rx0 || ry1 <= ry0) return null;
  return { x: rx0, y: ry0, w: rx1 - rx0, h: ry1 - ry0 };
}

const COLORS = {
  unexplored: "#080b0c",
  passable: "#263326",
  impassable: "#252b2d",
  ore: "#b88732",
  own: "#5f8996",
  enemy: "#a35a4e",
  selected: "#d7bb63",
};

export interface RenderOptions {
  waypoints?: Map<number, [number, number]>;
  placementMode?: string | null;
  placementCursor?: [number, number] | null;
  drawMinimap?: boolean;
}

export class Renderer {
  camera = new Camera();

  draw(
    ctx: CanvasRenderingContext2D,
    world: World,
    selection: Set<number>,
    w: number,
    h: number,
    opts: RenderOptions = {},
  ): void {
    this.camera.setViewport(w, h);
    // The canvas is a display surface for integer-cell sprites, never a
    // smoothed illustration.
    ctx.imageSmoothingEnabled = false;

    // 1. Clear background to dark void
    ctx.fillStyle = COLORS.unexplored;
    ctx.fillRect(0, 0, w, h);

    const cam = this.camera;
    const x0 = Math.max(0, Math.floor(cam.worldX(0)));
    const y0 = Math.max(0, Math.floor(cam.worldY(0)));
    const x1 = Math.min(MAP - 1, Math.ceil(cam.worldX(w)));
    const y1 = Math.min(MAP - 1, Math.ceil(cam.worldY(h)));

    // 2. Terrain + Fog of War
    for (let ty = y0; ty <= y1; ty++) {
      for (let tx = x0; tx <= x1; tx++) {
        const idx = ty * MAP + tx;
        const isVis = world.visible.has(idx);
        const isExp = world.explored.has(idx);
        if (!isVis && !isExp) continue;

        const px = cam.screenX(tx);
        const py = cam.screenY(ty);
        const size = cam.zoom + 0.5;
        const isPassable = world.passable[idx] ?? true;

        if (isPassable) {
          // Rock-adjacency lets the tile draw its own contact shadow where
          // formations meet soil — grounds the world visually.
          drawPassableTile(ctx, tx, ty, px, py, size, !isVis, {
            n: !(world.passable[idx - MAP] ?? true),
            s: !(world.passable[idx + MAP] ?? true),
            w: tx > 0 ? !(world.passable[idx - 1] ?? true) : false,
            e: tx < MAP - 1 ? !(world.passable[idx + 1] ?? true) : false,
          });
        } else {
          drawImpassableTile(ctx, tx, ty, px, py, size, !isVis);
        }
      }
    }

    // 3. Ore Fields
    for (const t of world.oreTiles.values()) {
      const px = cam.screenX(t.x);
      const py = cam.screenY(t.y);
      const size = cam.zoom;
      if (px > w || py > h || px + size < 0 || py + size < 0) continue;
      drawOreDeposit(ctx, px, py, size, t.amount, world.tick);
    }

    // 4. Ground Layer FX: Scorch craters, tracks, wreckage
    fx.drawGroundLayer(ctx, cam, w, h);

    // 5. Waypoint destination lines
    if (opts.waypoints && selection.size > 0) {
      this.drawWaypoints(ctx, world, selection, opts.waypoints);
    }

    // 6. Entities: Buildings first, then ground units, then air units
    const drawList = [...world.entities.values()].sort((a, b) => {
      const aTier = isAirUnit(a) ? 2 : isUnit(a) ? 1 : 0;
      const bTier = isAirUnit(b) ? 2 : isUnit(b) ? 1 : 0;
      if (aTier !== bTier) return aTier - bTier;
      return a.id - b.id;
    });

    for (const e of drawList) {
      this.drawEntity(ctx, world, e, selection, w, h);
    }

    // 7. Air Layer FX: Projectiles, lasers, explosions, particles
    fx.drawAirLayer(ctx, cam, w, h);

    // 8. Building placement ghost
    if (opts.placementMode && opts.placementCursor) {
      this.drawPlacementGhost(ctx, opts.placementMode, opts.placementCursor, world);
    }

    // 9. Optional on-canvas Minimap
    if (opts.drawMinimap) {
      this.drawMinimap(ctx, world, selection, w, h);
    }
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

    const heading = world.heading(e.id);
    const firingAge = fx.getFiringAge(e.id, world.tick);

    if (isUnit(e)) {
      fx.recordVehicleMovement(e.id, e.kind, p.x, p.y, heading);
      const isMoving = world.isMoving(e.id);
      drawUnitSprite(ctx, e.kind, px, py, z, e.owner, heading, world.tick, isStale, 0, firingAge, isMoving);
    } else {
      // Battle damage: heavily damaged structures vent smoke and embers.
      if (
        e.hp > 0 &&
        e.hp < e.maxHp * 0.5 &&
        (world.tick + e.id) % 5 === 0 &&
        !isStale
      ) {
        const hurt = 1 - e.hp / Math.max(1, e.maxHp);
        fx.particles.push({
          x: p.x + (Math.random() - 0.5) * 0.35,
          y: p.y - 0.25 - Math.random() * 0.2,
          vx: (Math.random() - 0.5) * 0.3,
          vy: -0.7 - Math.random() * 0.6,
          life: 0.8 + Math.random() * 0.5,
          maxLife: 1.3,
          size: 3 + hurt * 2,
          color: Math.random() < 0.75 ? "#1c1c20" : "#c2410c",
          alpha: 0.75,
        });
      }
      drawBuildingSprite(
        ctx,
        e.kind,
        px,
        py,
        z,
        e.owner,
        heading,
        world.tick,
        isStale,
        e.progress ?? 0,
        e.buildTime ?? 0,
        firingAge,
      );
    }

    if (isSelected) {
      const reticleSize = isUnit(e) ? z * 0.9 : z * 1.15;
      drawSelectionReticle(ctx, px, py, reticleSize, world.tick);
    }

    if (e.owner === 0 && e.maxHp > 0) {
      const barSize = isUnit(e) ? z * 0.8 : z * 1.1;
      drawHealthBar(ctx, px, py, barSize, e.hp, e.maxHp);
    }

    ctx.restore();
  }

  private drawWaypoints(
    ctx: CanvasRenderingContext2D,
    world: World,
    selection: Set<number>,
    waypoints: Map<number, [number, number]>,
  ): void {
    const cam = this.camera;

    for (const id of selection) {
      const e = world.entities.get(id);
      const wp = waypoints.get(id) ?? (e?.rally ? [e.rally[0], e.rally[1]] : undefined);
      if (!wp) continue;

      const p = world.pos(id);
      const fromX = cam.screenX(p.x);
      const fromY = cam.screenY(p.y);
      const toX = cam.screenX(wp[0] + 0.5);
      const toY = cam.screenY(wp[1] + 0.5);

      const isBuilding = e && BUILDING_KINDS.has(e.kind);

      ctx.save();
      if (isBuilding) {
        // Gold dashed line for building rally point
        ctx.strokeStyle = "rgba(250, 204, 21, 0.75)";
        ctx.lineWidth = 1.5;
        ctx.setLineDash([4, 4]);
        ctx.lineDashOffset = -(world.tick * 0.5) % 8;
        ctx.beginPath();
        ctx.moveTo(fromX, fromY);
        ctx.lineTo(toX, toY);
        ctx.stroke();
        ctx.restore();

        // Custom pixel-art tactical rally flag & beacon pip
        this.drawRallyFlag(ctx, toX, toY, cam.zoom, world.tick);
      } else {
        // Cyan dashed line for unit move waypoints
        ctx.strokeStyle = "rgba(6, 182, 212, 0.65)";
        ctx.lineWidth = 1.5;
        ctx.setLineDash([4, 4]);
        ctx.lineDashOffset = -(world.tick * 0.5) % 8;
        ctx.beginPath();
        ctx.moveTo(fromX, fromY);
        ctx.lineTo(toX, toY);
        ctx.stroke();

        const r = Math.max(4, Math.floor(cam.zoom * 0.25));
        ctx.strokeStyle = "#ffe27a";
        ctx.lineWidth = 1.5;
        ctx.setLineDash([]);
        ctx.strokeRect(Math.floor(toX - r), Math.floor(toY - r), r * 2, r * 2);
        ctx.fillStyle = "#06b6d4";
        ctx.fillRect(Math.floor(toX - 2), Math.floor(toY - 2), 4, 4);
        ctx.restore();
      }
    }
  }

  private drawRallyFlag(ctx: CanvasRenderingContext2D, toX: number, toY: number, z: number, tick: number): void {
  ctx.save();
  const s = Math.max(12, Math.floor(z * 0.45));

  // 1. Ground target ring & beacon pip
  const pulse = (Math.sin(tick * 0.3) + 1) * 0.5;
  ctx.strokeStyle = `rgba(234, 179, 8, ${0.4 + pulse * 0.4})`;
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.arc(toX, toY, Math.max(4, 5 + pulse * 3), 0, Math.PI * 2);
  ctx.stroke();

  // Center target pip
  ctx.fillStyle = "#facc15";
  ctx.fillRect(Math.floor(toX - 2), Math.floor(toY - 2), 4, 4);

  // 2. Steel flagpole mast (extends upwards from target)
  const mastH = Math.floor(s * 1.3);
  ctx.fillStyle = "#64748b";
  ctx.fillRect(Math.floor(toX - 1), Math.floor(toY - mastH), 2, mastH);
  ctx.fillStyle = "#cbd5e1";
  ctx.fillRect(Math.floor(toX - 1), Math.floor(toY - mastH), 1, mastH);

  // 3. Top flagpole beacon light (pulsing red)
  const beacon = (tick % 6) < 3;
  ctx.fillStyle = beacon ? "#ef4444" : "#991b1b";
  ctx.fillRect(Math.floor(toX - 2), Math.floor(toY - mastH - 3), 4, 3);
  if (beacon) {
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(Math.floor(toX - 1), Math.floor(toY - mastH - 2), 2, 1);
  }

  // 4. Triangular fluttering golden rally flag
  const wave = Math.sin(tick * 0.4) > 0 ? 1 : 0;
  const flagW = Math.max(10, Math.floor(s * 0.75));
  const flagTop = Math.floor(toY - mastH);

  ctx.fillStyle = "#eab308";
  ctx.fillRect(Math.floor(toX + 1), flagTop, flagW, 2);
  ctx.fillRect(Math.floor(toX + 1), flagTop + 2, flagW - 2 - wave, 2);
  ctx.fillRect(Math.floor(toX + 1), flagTop + 4, flagW - 5 - wave, 2);
  ctx.fillRect(Math.floor(toX + 1), flagTop + 6, Math.max(2, flagW - 8), 2);

  // Gold highlight & chevron stripe on flag
  ctx.fillStyle = "#fef08a";
  ctx.fillRect(Math.floor(toX + 2), flagTop + 1, flagW - 2, 1);
  ctx.fillStyle = "#ca8a04";
  ctx.fillRect(Math.floor(toX + 3), flagTop + 2, 2, 4);

  ctx.restore();
}

  private drawPlacementGhost(
    ctx: CanvasRenderingContext2D,
    btype: string,
    cursor: [number, number],
    world: World,
  ): void {
    const cam = this.camera;
    const px = cam.screenX(cursor[0] + 0.5);
    const py = cam.screenY(cursor[1] + 0.5);
    const placable = isBuildingPlacable(btype, cursor, world);

    ctx.save();
    ctx.globalAlpha = placable ? 0.75 : 0.45;
    drawBuildingSprite(ctx, btype, px, py, cam.zoom, 0, 0, world.tick);

    const rx = cam.screenX(cursor[0]);
    const ry = cam.screenY(cursor[1]);

    if (placable) {
      ctx.fillStyle = "rgba(34, 197, 94, 0.18)";
      ctx.fillRect(rx, ry, cam.zoom, cam.zoom);
      ctx.strokeStyle = "#22c55e";
      ctx.lineWidth = 2;
      ctx.strokeRect(rx, ry, cam.zoom, cam.zoom);
    } else {
      ctx.fillStyle = "rgba(239, 68, 68, 0.25)";
      ctx.fillRect(rx, ry, cam.zoom, cam.zoom);
      ctx.strokeStyle = "#ef4444";
      ctx.lineWidth = 2;
      ctx.strokeRect(rx, ry, cam.zoom, cam.zoom);

      // Red X across unplacable tile
      ctx.beginPath();
      ctx.moveTo(rx + 2, ry + 2);
      ctx.lineTo(rx + cam.zoom - 2, ry + cam.zoom - 2);
      ctx.moveTo(rx + cam.zoom - 2, ry + 2);
      ctx.lineTo(rx + 2, ry + cam.zoom - 2);
      ctx.stroke();
    }
    ctx.restore();
  }

  drawMinimap(
    ctx: CanvasRenderingContext2D,
    world: World,
    selection: Set<number>,
    w: number,
    h: number,
  ): void {
    const s = 3.5;
    const ox = 12;
    const oy = h - MAP * s - 12;

    ctx.fillStyle = "rgba(10, 14, 18, 0.92)";
    ctx.fillRect(ox - 3, oy - 3, MAP * s + 6, MAP * s + 6);
    ctx.strokeStyle = "#334155";
    ctx.lineWidth = 1.5;
    ctx.strokeRect(ox - 3, oy - 3, MAP * s + 6, MAP * s + 6);

    for (let ty = 0; ty < MAP; ty++) {
      for (let tx = 0; tx < MAP; tx++) {
        const idx = ty * MAP + tx;
        if (world.visible.has(idx)) {
          ctx.fillStyle = world.passable[idx] ? COLORS.passable : COLORS.impassable;
        } else if (world.explored.has(idx)) {
          ctx.fillStyle = world.passable[idx] ? "#172119" : "#1b2022";
        } else {
          continue;
        }
        ctx.fillRect(ox + tx * s, oy + ty * s, s, s);
      }
    }

    ctx.fillStyle = COLORS.ore;
    for (const t of world.oreTiles.values()) {
      if (t.amount > 0) {
        ctx.fillRect(ox + t.x * s, oy + t.y * s, s, s);
      }
    }

    for (const e of world.entities.values()) {
      const p = world.pos(e.id);
      const isSel = selection.has(e.id);
      ctx.fillStyle = isSel ? COLORS.selected : e.owner === 0 ? COLORS.own : COLORS.enemy;
      const dotSize = isUnit(e) ? 2 : 3;
      ctx.fillRect(ox + Math.floor(p.x * s) - 1, oy + Math.floor(p.y * s) - 1, dotSize, dotSize);
    }

    // Accurate Viewport Box
    const vr = cameraViewRect(this.camera, w, h);
    if (vr) {
      ctx.strokeStyle = "rgba(255, 226, 122, 0.85)";
      ctx.lineWidth = 1.5;
      ctx.strokeRect(ox + vr.x * s, oy + vr.y * s, vr.w * s, vr.h * s);
      ctx.fillStyle = "rgba(255, 226, 122, 0.1)";
      ctx.fillRect(ox + vr.x * s, oy + vr.y * s, vr.w * s, vr.h * s);
    }
  }

  minimapToWorld(sx: number, sy: number, _w: number, h: number): [number, number] | null {
    const s = 3.5;
    const ox = 12;
    const oy = h - MAP * s - 12;
    if (sx >= ox && sx <= ox + MAP * s && sy >= oy && sy <= oy + MAP * s) {
      return [(sx - ox) / s, (sy - oy) / s];
    }
    return null;
  }
}

/** Render tactical Radar Surveillance panel */
export function drawRadar(
  ctx: CanvasRenderingContext2D,
  world: World,
  selection: Set<number>,
  camera: Camera,
  w: number,
  h: number,
): void {
  ctx.imageSmoothingEnabled = false;
  ctx.fillStyle = "#080b0c";
  ctx.fillRect(0, 0, w, h);

  const s = Math.min(w, h) / MAP;
  const ox = (w - MAP * s) / 2;
  const oy = (h - MAP * s) / 2;

  // 1. Terrain tiles
  for (let ty = 0; ty < MAP; ty++) {
    for (let tx = 0; tx < MAP; tx++) {
      const idx = ty * MAP + tx;
      if (world.visible.has(idx)) {
        ctx.fillStyle = world.passable[idx] ? COLORS.passable : COLORS.impassable;
      } else if (world.explored.has(idx)) {
        ctx.fillStyle = world.passable[idx] ? "#172119" : "#1b2022";
      } else {
        continue;
      }
      ctx.fillRect(ox + tx * s, oy + ty * s, Math.ceil(s), Math.ceil(s));
    }
  }

  // 2. Ore fields
  ctx.fillStyle = COLORS.ore;
  for (const t of world.oreTiles.values()) {
    if (t.amount > 0) {
      ctx.fillRect(ox + t.x * s, oy + t.y * s, Math.max(2, s), Math.max(2, s));
    }
  }

  // 3. Entities
  for (const e of world.entities.values()) {
    const p = world.pos(e.id);
    const isSel = selection.has(e.id);
    ctx.fillStyle = isSel ? COLORS.selected : e.owner === 0 ? COLORS.own : COLORS.enemy;
    const dotSize = isUnit(e) ? Math.max(2, s) : Math.max(3, s * 1.5);
    ctx.fillRect(ox + p.x * s - dotSize / 2, oy + p.y * s - dotSize / 2, dotSize, dotSize);
  }

  // 3b. 8×8 sector grid (the same sectors the AI's army head targets, plan
  // §8: "Minimap with sector grid") — lets players read AI target choices.
  ctx.strokeStyle = "rgba(56, 189, 248, 0.22)";
  ctx.lineWidth = 1;
  for (let i = 1; i < 8; i++) {
    const gx = ox + i * (MAP / 8) * s;
    const gy = oy + i * (MAP / 8) * s;
    ctx.beginPath();
    ctx.moveTo(gx, oy);
    ctx.lineTo(gx, oy + MAP * s);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(ox, gy);
    ctx.lineTo(ox + MAP * s, gy);
    ctx.stroke();
  }

  // 4. Accurate Camera Viewport Box
  const vr = cameraViewRect(camera, window.innerWidth, window.innerHeight);
  if (vr) {
    ctx.strokeStyle = "rgba(255, 226, 122, 0.9)";
    ctx.lineWidth = 1.5;
    ctx.strokeRect(ox + vr.x * s, oy + vr.y * s, Math.max(4, vr.w * s), Math.max(4, vr.h * s));
    ctx.fillStyle = "rgba(255, 226, 122, 0.12)";
    ctx.fillRect(ox + vr.x * s, oy + vr.y * s, Math.max(4, vr.w * s), Math.max(4, vr.h * s));
  }

  // 5. Radar sweep animation
  const sweepAngle = (performance.now() * 0.002) % (Math.PI * 2);
  const cx = ox + (MAP * s) / 2;
  const cy = oy + (MAP * s) / 2;
  const sweepLen = (MAP * s) * 0.7;
  ctx.strokeStyle = "rgba(6, 182, 212, 0.4)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(cx, cy);
  ctx.lineTo(cx + Math.cos(sweepAngle) * sweepLen, cy + Math.sin(sweepAngle) * sweepLen);
  ctx.stroke();

  // 6. Grid lines
  ctx.strokeStyle = "rgba(71, 85, 105, 0.35)";
  ctx.lineWidth = 1;
  ctx.strokeRect(ox, oy, MAP * s, MAP * s);
  ctx.beginPath();
  ctx.moveTo(ox, cy);
  ctx.lineTo(ox + MAP * s, cy);
  ctx.moveTo(cx, oy);
  ctx.lineTo(cx, oy + MAP * s);
  ctx.stroke();
}

export function isUnit(e: Entity): boolean {
  return UNIT_KINDS.has(e.kind);
}

export function isAirUnit(e: Entity): boolean {
  return e.kind === "Gunship" || e.kind === "Interceptor";
}

export function isBuildingPlacable(
  btype: string,
  tile: [number, number],
  world: World,
): boolean {
  const [tx, ty] = tile;
  if (tx < 0 || tx >= MAP || ty < 0 || ty >= MAP) return false;
  const idx = ty * MAP + tx;
  if (world.passable.length > 0 && !world.passable[idx]) return false;

  // Check if tile has an existing building
  for (const e of world.entities.values()) {
    if (BUILDING_KINDS.has(e.kind)) {
      if (Math.floor(e.x) === tx && Math.floor(e.y) === ty) {
        return false;
      }
    }
  }

  // Check if tile has ore
  const ore = world.oreTiles.get(`${tx},${ty}`);
  if (ore && ore.amount > 0) return false;

  // Check distance to nearest own building (within 5 tiles Euclidean)
  const PLACE_RADIUS_SQ = 25; // 5^2
  let nearOwn = false;
  for (const b of world.ownBuildings) {
    const bx = Math.floor(b.x);
    const by = Math.floor(b.y);
    const d2 = (bx - tx) * (bx - tx) + (by - ty) * (by - ty);
    if (d2 <= PLACE_RADIUS_SQ) {
      nearOwn = true;
      break;
    }
  }
  if (!nearOwn && world.ownBuildings.length > 0) {
    return false;
  }

  // Tech tree gates: TechLab & Airfield need a Factory; Radar & TeslaCoil
  // are the second tier and need the TechLab itself.
  if (btype === "TechLab" || btype === "Airfield") {
    const hasFactory = world.ownBuildings.some((b) => b.kind === "Factory");
    if (!hasFactory) return false;
  }
  if (btype === "Radar" || btype === "TeslaCoil") {
    const hasLab = world.ownBuildings.some((b) => b.kind === "TechLab");
    if (!hasLab) return false;
  }

  // Check if player has enough ore
  const cost = BUILD_COSTS[btype] ?? 0;
  if (world.ore < cost) return false;

  return true;
}

