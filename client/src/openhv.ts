// OpenHV content bridge.
//
// The importer places selected OpenHV PNG sheets under /openhv during local
// setup. Missing or not-yet-loaded files deliberately return false so callers
// can keep their deterministic procedural fallback on the same frame.

export interface OpenHvSprite {
  path: string;
  frameWidth: number;
  frameHeight: number;
  frameCount: number;
}

export const OPENHV_SPRITES = {
  infantry: { path: "sprites/infantry/rifleman.png", frameWidth: 20, frameHeight: 20, frameCount: 8 },
  tank: { path: "sprites/vehicles/mbt.png", frameWidth: 33, frameHeight: 26, frameCount: 16 },
  artillery: { path: "sprites/vehicles/artillery.png", frameWidth: 24, frameHeight: 24, frameCount: 8 },
  harvester: { path: "sprites/vehicles/miner.png", frameWidth: 20, frameHeight: 20, frameCount: 8 },

  hq: { path: "sprites/buildings/base.png", frameWidth: 60, frameHeight: 60, frameCount: 8 },
  refinery: { path: "sprites/buildings/extractor.png", frameWidth: 40, frameHeight: 56, frameCount: 10 },
  barracks: { path: "sprites/buildings/outpost.png", frameWidth: 20, frameHeight: 50, frameCount: 5 },
  factory: { path: "sprites/buildings/factory.png", frameWidth: 37, frameHeight: 42, frameCount: 12 },
  techLab: { path: "sprites/buildings/techcenter.png", frameWidth: 36, frameHeight: 52, frameCount: 12 },
  turret: { path: "sprites/buildings/turret.png", frameWidth: 47, frameHeight: 40, frameCount: 18 },

  grass: { path: "sprites/terrain/grass.png", frameWidth: 20, frameHeight: 20, frameCount: 34 },
  rock1: { path: "sprites/terrain/rock1.png", frameWidth: 20, frameHeight: 20, frameCount: 1 },
  rock2: { path: "sprites/terrain/rock2.png", frameWidth: 20, frameHeight: 20, frameCount: 1 },
  ore: { path: "sprites/gold.png", frameWidth: 20, frameHeight: 20, frameCount: 3 },

  scorch: { path: "sprites/effects/smudges1.png", frameWidth: 20, frameHeight: 20, frameCount: 3 },
  explosionSmall: { path: "sprites/effects/expsmall.png", frameWidth: 20, frameHeight: 20, frameCount: 7 },
  explosionMedium: { path: "sprites/effects/explosn.png", frameWidth: 20, frameHeight: 20, frameCount: 9 },
  explosionLarge: { path: "sprites/effects/explobig.png", frameWidth: 40, frameHeight: 40, frameCount: 13 },
  smoke: { path: "sprites/effects/smoke.png", frameWidth: 20, frameHeight: 20, frameCount: 6 },
  sparks: { path: "sprites/effects/sparks1.png", frameWidth: 20, frameHeight: 20, frameCount: 7 },
  projectile: { path: "sprites/effects/bullet1.png", frameWidth: 11, frameHeight: 11, frameCount: 8 },
} as const satisfies Record<string, OpenHvSprite>;

export type OpenHvSpriteKey = keyof typeof OPENHV_SPRITES;

const imageCache = new Map<string, HTMLImageElement | null>();

function imageFor(sprite: OpenHvSprite): HTMLImageElement | null {
  if (typeof document === "undefined" || typeof Image === "undefined") return null;

  const cached = imageCache.get(sprite.path);
  if (cached !== undefined) return cached;

  const image = document.createElement("img");
  image.decoding = "async";
  image.src = `/openhv/${sprite.path}`;
  imageCache.set(sprite.path, image);
  return image;
}

function frameIndex(sprite: OpenHvSprite, frame: number): number {
  const count = Math.max(1, sprite.frameCount);
  return ((Math.floor(frame) % count) + count) % count;
}

/**
 * Draw one OpenRA/OpenHV sprite-sheet frame at a requested source-pixel scale.
 * Returns false while the optional local asset is absent or still loading.
 */
export function drawOpenHvFrame(
  ctx: CanvasRenderingContext2D,
  sprite: OpenHvSprite,
  cx: number,
  cy: number,
  requestedScale: number,
  frame: number = 0,
): boolean {
  const image = imageFor(sprite);
  if (!image || !image.complete || image.naturalWidth < sprite.frameWidth || image.naturalHeight < sprite.frameHeight) return false;
  if (typeof ctx.drawImage !== "function") return false;

  const index = frameIndex(sprite, frame);
  const columns = Math.max(1, Math.floor(image.naturalWidth / sprite.frameWidth));
  const sx = (index % columns) * sprite.frameWidth;
  const sy = Math.floor(index / columns) * sprite.frameHeight;
  if (sx + sprite.frameWidth > image.naturalWidth || sy + sprite.frameHeight > image.naturalHeight) return false;

  const scale = Math.max(0.1, requestedScale);
  const dw = Math.max(1, Math.round(sprite.frameWidth * scale));
  const dh = Math.max(1, Math.round(sprite.frameHeight * scale));

  ctx.save();
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(
    image,
    sx,
    sy,
    sprite.frameWidth,
    sprite.frameHeight,
    Math.floor(cx - dw / 2),
    Math.floor(cy - dh / 2),
    dw,
    dh,
  );
  ctx.restore();
  return true;
}

/** Draw a sprite sheet frame using a desired on-screen width in pixels. */
export function drawOpenHvSprite(
  ctx: CanvasRenderingContext2D,
  key: OpenHvSpriteKey,
  cx: number,
  cy: number,
  targetWidth: number,
  frame: number = 0,
): boolean {
  const sprite = OPENHV_SPRITES[key];
  return drawOpenHvFrame(ctx, sprite, cx, cy, targetWidth / sprite.frameWidth, frame);
}

/** OpenRA's eight facings are N, NW, W, SW, S, SE, E, NE. */
export function openHvFacing(angle: number): number {
  const octant = Math.round(angle / (Math.PI / 4));
  const normalized = ((octant % 8) + 8) % 8;
  return [6, 5, 4, 3, 2, 1, 0, 7][normalized];
}

/** Pick a stable animation frame without coupling the renderer to wall-clock time. */
export function openHvAnimationFrame(key: OpenHvSpriteKey, tick: number, start = 0, length?: number): number {
  const sprite = OPENHV_SPRITES[key];
  const available = Math.max(1, length ?? sprite.frameCount - start);
  return start + Math.floor(tick / 4) % available;
}
