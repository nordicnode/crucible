// HUD overlay primitives drawn on the tactical canvas above entities:
// selection brackets and segmented armor-integrity bars.
// Split from sprites.ts so screen-space UI stays independent of world art.

function pRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, color: string): void {
  ctx.fillStyle = color;
  ctx.fillRect(Math.floor(x), Math.floor(y), Math.max(1, Math.floor(w)), Math.max(1, Math.floor(h)));
}

function pStroke(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, color: string): void {
  ctx.strokeStyle = color;
  ctx.lineWidth = 1;
  ctx.strokeRect(Math.floor(x) + 0.5, Math.floor(y) + 0.5, Math.floor(w) - 1, Math.floor(h) - 1);
}

/** Animated corner-bracket selection reticle. The brackets breathe on a slow
 *  pulse so an active selection feels alive without visual noise, and thin
 *  midpoint ticks form a secondary tracking ring. */
export function drawSelectionReticle(
  ctx: CanvasRenderingContext2D,
  px: number,
  py: number,
  size: number,
  tick: number = 0,
): void {
  const pulse = (Math.sin(tick * 0.22) + 1) / 2;
  const r = Math.floor(size * (0.55 + pulse * 0.07));
  const len = Math.max(4, Math.floor(r * 0.38));

  ctx.strokeStyle = "#ffd75e";
  ctx.lineWidth = 2;

  ctx.beginPath();
  // Top-left corner
  ctx.moveTo(px - r, py - r + len);
  ctx.lineTo(px - r, py - r);
  ctx.lineTo(px - r + len, py - r);
  // Top-right corner
  ctx.moveTo(px + r - len, py - r);
  ctx.lineTo(px + r, py - r);
  ctx.lineTo(px + r, py - r + len);
  // Bottom-left corner
  ctx.moveTo(px - r, py + r - len);
  ctx.lineTo(px - r, py + r);
  ctx.lineTo(px - r + len, py + r);
  // Bottom-right corner
  ctx.moveTo(px + r - len, py + r);
  ctx.lineTo(px + r, py + r);
  ctx.lineTo(px + r, py + r - len);
  ctx.stroke();

  // Thin midpoint ticks on each face
  ctx.strokeStyle = "rgba(255, 215, 94, 0.5)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(px - 2, py - r - 3);
  ctx.lineTo(px + 2, py - r - 3);
  ctx.moveTo(px - 2, py + r + 3);
  ctx.lineTo(px + 2, py + r + 3);
  ctx.moveTo(px - r - 3, py - 2);
  ctx.lineTo(px - r - 3, py + 2);
  ctx.moveTo(px + r + 3, py - 2);
  ctx.lineTo(px + r + 3, py + 2);
  ctx.stroke();
}

/** Segmented C&C-style integrity pip bar: each cell reads as one discrete
 *  chunk of armor, legible even at low zoom. A bright leading edge marks the
 *  exact damage frontier. */
export function drawHealthBar(
  ctx: CanvasRenderingContext2D,
  px: number,
  py: number,
  size: number,
  hp: number,
  maxHp: number,
): void {
  const segW = 3;
  const gap = 1;
  const w = Math.floor(size * 0.85);
  const segs = Math.max(4, Math.floor((w + gap) / (segW + gap)));
  const totalW = segs * (segW + gap) - gap;
  const x = Math.floor(px - totalW / 2);
  const y = Math.floor(py - size * 0.62 - 6);

  const pct = Math.max(0, Math.min(1, hp / Math.max(1, maxHp)));
  const lit = Math.round(pct * segs);
  const barCol = pct > 0.5 ? "#3ecf6e" : pct > 0.25 ? "#eab308" : "#ef4444";

  pRect(ctx, x - 1, y - 1, totalW + 2, 5, "#05070a");
  pStroke(ctx, x - 1, y - 1, totalW + 2, 5, "#2c3947");
  for (let i = 0; i < segs; i++) {
    const sx = x + i * (segW + gap);
    if (i < lit) {
      pRect(ctx, sx, y, segW, 3, barCol);
      if (i === lit - 1) pRect(ctx, sx + segW - 1, y, 1, 3, "#ffffff");
    } else {
      pRect(ctx, sx, y, segW, 3, "#101720");
    }
  }
}
