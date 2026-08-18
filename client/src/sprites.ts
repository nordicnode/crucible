// High-detail procedural vector / Canvas 2D sprite rendering system for Crucible.
// Renders crisp, scalable tactical sprites for terrain, buildings, units, and FX.

export interface TeamPalette {
  primary: string;
  primaryLight: string;
  primaryDark: string;
  accent: string;
  glow: string;
}

export const TEAM_BLUE: TeamPalette = {
  primary: "#2563eb",
  primaryLight: "#60a5fa",
  primaryDark: "#1d4ed8",
  accent: "#93c5fd",
  glow: "rgba(59, 130, 246, 0.6)",
};

export const TEAM_RED: TeamPalette = {
  primary: "#dc2626",
  primaryLight: "#f87171",
  primaryDark: "#b91c1c",
  accent: "#fca5a5",
  glow: "rgba(239, 68, 68, 0.6)",
};

export const TEAM_STALE: TeamPalette = {
  primary: "#4b5563",
  primaryLight: "#9ca3af",
  primaryDark: "#374151",
  accent: "#d1d5db",
  glow: "rgba(156, 163, 175, 0.3)",
};

export function getTeamPalette(owner: number, isStale: boolean = false): TeamPalette {
  if (isStale) return TEAM_STALE;
  return owner === 0 ? TEAM_BLUE : TEAM_RED;
}

// ---------------------------------------------------------------------------
// Hash helpers for deterministic tile variations
// ---------------------------------------------------------------------------

function tileHash(tx: number, ty: number): number {
  let h = (tx * 374761393 + ty * 668265263) ^ 0x5bf03635;
  h = (h ^ (h >> 13)) * 1274126143;
  return (h ^ (h >> 16)) >>> 0;
}

// ---------------------------------------------------------------------------
// Terrain Rendering
// ---------------------------------------------------------------------------

export function drawPassableTile(
  ctx: CanvasRenderingContext2D,
  tx: number,
  ty: number,
  px: number,
  py: number,
  size: number,
  isExploredOnly: boolean,
): void {
  const h = tileHash(tx, ty);
  const variant = h % 4;

  if (isExploredOnly) {
    ctx.fillStyle = "#0c1510";
    ctx.fillRect(px, py, size, size);
    ctx.strokeStyle = "#080e0b";
    ctx.lineWidth = 0.5;
    ctx.strokeRect(px, py, size, size);
    return;
  }

  // Base ground: tactical dark slate/moss ground
  const baseColors = ["#16281c", "#182c1f", "#14251a", "#1a3022"];
  ctx.fillStyle = baseColors[variant];
  ctx.fillRect(px, py, size, size);

  // Subtle grid seam
  ctx.strokeStyle = "rgba(10, 20, 14, 0.5)";
  ctx.lineWidth = 0.5;
  ctx.strokeRect(px, py, size, size);

  // Micro-texture details based on variant
  if (size >= 10) {
    if (variant === 1) {
      // Tech floor panel seam
      ctx.strokeStyle = "rgba(45, 80, 55, 0.25)";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(px + 2, py + size / 2);
      ctx.lineTo(px + size - 2, py + size / 2);
      ctx.stroke();
    } else if (variant === 2) {
      // Corner rivet dots
      ctx.fillStyle = "rgba(40, 75, 50, 0.4)";
      const d = Math.max(1, size * 0.08);
      ctx.fillRect(px + 2, py + 2, d, d);
      ctx.fillRect(px + size - 2 - d, py + size - 2 - d, d, d);
    } else if (variant === 3) {
      // Subtle terrain grit
      ctx.fillStyle = "rgba(50, 90, 60, 0.25)";
      ctx.fillRect(px + size * 0.3, py + size * 0.3, size * 0.15, size * 0.15);
      ctx.fillRect(px + size * 0.65, py + size * 0.6, size * 0.12, size * 0.12);
    }
  }
}

export function drawImpassableTile(
  ctx: CanvasRenderingContext2D,
  tx: number,
  ty: number,
  px: number,
  py: number,
  size: number,
  isExploredOnly: boolean,
): void {
  const h = tileHash(tx, ty);

  if (isExploredOnly) {
    ctx.fillStyle = "#121519";
    ctx.fillRect(px, py, size, size);
    ctx.strokeStyle = "#0d0f12";
    ctx.lineWidth = 0.5;
    ctx.strokeRect(px, py, size, size);
    return;
  }

  // Impassable rock / armored barrier
  ctx.fillStyle = "#22272e";
  ctx.fillRect(px, py, size, size);

  // 3D Bevel: top and left highlight
  ctx.fillStyle = "#38414e";
  ctx.beginPath();
  ctx.moveTo(px, py);
  ctx.lineTo(px + size, py);
  ctx.lineTo(px + size - 2, py + 2);
  ctx.lineTo(px + 2, py + 2);
  ctx.lineTo(px + 2, py + size - 2);
  ctx.lineTo(px, py + size);
  ctx.closePath();
  ctx.fill();

  // 3D Bevel: bottom and right shadow
  ctx.fillStyle = "#14181c";
  ctx.beginPath();
  ctx.moveTo(px + size, py);
  ctx.lineTo(px + size, py + size);
  ctx.lineTo(px, py + size);
  ctx.lineTo(px + 2, py + size - 2);
  ctx.lineTo(px + size - 2, py + size - 2);
  ctx.lineTo(px + size - 2, py + 2);
  ctx.closePath();
  ctx.fill();

  // Inner rock face texture / fissure
  if (size >= 12) {
    const v = h % 3;
    ctx.fillStyle = "#2c333d";
    ctx.fillRect(px + 3, py + 3, size - 6, size - 6);

    ctx.strokeStyle = "#1a1f26";
    ctx.lineWidth = 1;
    ctx.beginPath();
    if (v === 0) {
      ctx.moveTo(px + 4, py + size * 0.4);
      ctx.lineTo(px + size * 0.6, py + size * 0.5);
      ctx.lineTo(px + size - 4, py + size * 0.8);
    } else if (v === 1) {
      ctx.moveTo(px + size * 0.5, py + 4);
      ctx.lineTo(px + size * 0.4, py + size * 0.6);
      ctx.lineTo(px + size * 0.7, py + size - 4);
    } else {
      ctx.moveTo(px + 4, py + size - 4);
      ctx.lineTo(px + size * 0.5, py + size * 0.5);
      ctx.lineTo(px + size - 4, py + 4);
    }
    ctx.stroke();
  }
}

// ---------------------------------------------------------------------------
// Ore Deposit Rendering
// ---------------------------------------------------------------------------

export function drawOreDeposit(
  ctx: CanvasRenderingContext2D,
  px: number,
  py: number,
  size: number,
  amount: number,
  tick: number,
): void {
  const cx = px + size * 0.5;
  const cy = py + size * 0.5;
  const scale = Math.min(1, Math.max(0.4, amount / 400));
  const s = size * 0.42 * scale;

  // Ambient gold ground glow
  const glowGrad = ctx.createRadialGradient(cx, cy, 0, cx, cy, s * 1.6);
  glowGrad.addColorStop(0, "rgba(234, 179, 8, 0.35)");
  glowGrad.addColorStop(0.7, "rgba(161, 98, 7, 0.12)");
  glowGrad.addColorStop(1, "rgba(0, 0, 0, 0)");
  ctx.fillStyle = glowGrad;
  ctx.beginPath();
  ctx.arc(cx, cy, s * 1.6, 0, Math.PI * 2);
  ctx.fill();

  // Subtle sparkle pulse based on tick
  const shimmer = 0.85 + Math.sin(tick * 0.08 + (cx % 10)) * 0.15;

  // Multi-crystal cluster coordinates [dx, dy, widthScale, heightScale]
  const crystals = [
    { dx: 0, dy: s * 0.1, w: s * 0.38, h: s * 0.95 },
    { dx: -s * 0.45, dy: s * 0.25, w: s * 0.3, h: s * 0.7 },
    { dx: s * 0.45, dy: s * 0.2, w: s * 0.32, h: s * 0.75 },
  ];

  if (scale > 0.7) {
    crystals.push({ dx: -s * 0.2, dy: -s * 0.3, w: s * 0.25, h: s * 0.6 });
    crystals.push({ dx: s * 0.25, dy: -s * 0.25, w: s * 0.24, h: s * 0.55 });
  }

  for (const c of crystals) {
    const x = cx + c.dx;
    const y = cy + c.dy;
    const w = c.w;
    const h = c.h;

    // Dark facet / outline base
    ctx.beginPath();
    ctx.moveTo(x, y - h);
    ctx.lineTo(x + w, y);
    ctx.lineTo(x, y + h * 0.4);
    ctx.lineTo(x - w, y);
    ctx.closePath();
    ctx.fillStyle = "#713f12";
    ctx.fill();
    ctx.strokeStyle = "#451a03";
    ctx.lineWidth = 1;
    ctx.stroke();

    // Right shaded facet
    ctx.beginPath();
    ctx.moveTo(x, y - h);
    ctx.lineTo(x + w, y);
    ctx.lineTo(x, y + h * 0.4);
    ctx.closePath();
    ctx.fillStyle = "#ca8a04";
    ctx.fill();

    // Left illuminated facet
    ctx.beginPath();
    ctx.moveTo(x, y - h);
    ctx.lineTo(x, y + h * 0.4);
    ctx.lineTo(x - w, y);
    ctx.closePath();
    ctx.fillStyle = "#eab308";
    ctx.fill();

    // Specular ridge highlight
    ctx.beginPath();
    ctx.moveTo(x, y - h + 1);
    ctx.lineTo(x - w * 0.2, y - h * 0.2);
    ctx.lineTo(x, y + h * 0.2);
    ctx.closePath();
    ctx.fillStyle = `rgba(254, 240, 138, ${shimmer})`;
    ctx.fill();
  }
}

// ---------------------------------------------------------------------------
// Building Sprites
// ---------------------------------------------------------------------------

export function drawBuildingSprite(
  ctx: CanvasRenderingContext2D,
  kind: string,
  px: number,
  py: number,
  zoom: number,
  owner: number,
  tick: number,
  isStale: boolean = false,
  progress: number = 0,
  buildTime: number = 0,
): void {
  const pal = getTeamPalette(owner, isStale);

  ctx.save();
  ctx.translate(px, py);

  switch (kind) {
    case "Hq":
      drawHq(ctx, zoom, pal, tick);
      break;
    case "Refinery":
      drawRefinery(ctx, zoom, pal, tick);
      break;
    case "Barracks":
      drawBarracks(ctx, zoom, pal, tick);
      break;
    case "Factory":
      drawFactory(ctx, zoom, pal, tick);
      break;
    case "TechLab":
      drawTechLab(ctx, zoom, pal, tick);
      break;
    case "Turret":
      drawTurret(ctx, zoom, pal, tick);
      break;
    default:
      // Fallback
      ctx.fillStyle = pal.primary;
      ctx.fillRect(-zoom * 0.4, -zoom * 0.4, zoom * 0.8, zoom * 0.8);
  }

  // Production progress mini-indicator on the building if producing
  if (buildTime > 0 && progress > 0) {
    const frac = Math.min(1, progress / buildTime);
    const bw = zoom * 0.7;
    const bh = Math.max(2, zoom * 0.08);
    const by = zoom * 0.45;
    ctx.fillStyle = "rgba(0, 0, 0, 0.7)";
    ctx.fillRect(-bw / 2, by, bw, bh);
    ctx.fillStyle = "#eab308";
    ctx.fillRect(-bw / 2, by, bw * frac, bh);
    ctx.strokeStyle = "rgba(255, 255, 255, 0.4)";
    ctx.lineWidth = 0.5;
    ctx.strokeRect(-bw / 2, by, bw, bh);
  }

  ctx.restore();
}

/** HQ: Large Command Citadel with armored bunker hull, comms dish & glowing command deck */
function drawHq(
  ctx: CanvasRenderingContext2D,
  z: number,
  pal: TeamPalette,
  tick: number,
): void {
  const r = z * 0.48;

  // Ground drop shadow
  ctx.fillStyle = "rgba(0, 0, 0, 0.5)";
  ctx.beginPath();
  drawChamferedRect(ctx, -r - 1, -r + 2, (r + 1) * 2, (r + 1) * 2, 4);
  ctx.fill();

  // Outer armored hull (octagonal / chamfered)
  ctx.fillStyle = "#1e242c";
  ctx.strokeStyle = "#333d4b";
  ctx.lineWidth = 1.5;
  drawChamferedRect(ctx, -r, -r, r * 2, r * 2, r * 0.25);
  ctx.fill();
  ctx.stroke();

  // Corner armor plating & rivets
  ctx.fillStyle = "#2d3748";
  const cr = r * 0.35;
  ctx.fillRect(-r + 1, -r + 1, cr, cr);
  ctx.fillRect(r - 1 - cr, -r + 1, cr, cr);
  ctx.fillRect(-r + 1, r - 1 - cr, cr, cr);
  ctx.fillRect(r - 1 - cr, r - 1 - cr, cr, cr);

  // Team-colored command deck wing plates
  ctx.fillStyle = pal.primary;
  ctx.fillRect(-r * 0.6, -r * 0.85, r * 1.2, r * 0.28);
  ctx.fillRect(-r * 0.6, r * 0.57, r * 1.2, r * 0.28);

  ctx.fillStyle = pal.primaryLight;
  ctx.fillRect(-r * 0.5, -r * 0.8, r * 1.0, 1.5);
  ctx.fillRect(-r * 0.5, r * 0.62, r * 1.0, 1.5);

  // Central command bridge dome
  const bridgeR = r * 0.45;
  ctx.fillStyle = "#111827";
  ctx.beginPath();
  ctx.arc(0, 0, bridgeR, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = "#475569";
  ctx.lineWidth = 1;
  ctx.stroke();

  // Pulsing central holographic core / comms beacon
  const pulse = 0.6 + Math.sin(tick * 0.15) * 0.4;
  ctx.fillStyle = pal.accent;
  ctx.beginPath();
  ctx.arc(0, 0, bridgeR * 0.5, 0, Math.PI * 2);
  ctx.fill();

  ctx.fillStyle = `rgba(255, 255, 255, ${pulse})`;
  ctx.beginPath();
  ctx.arc(0, 0, bridgeR * 0.25, 0, Math.PI * 2);
  ctx.fill();

  // Comms antenna dish
  const dishAngle = tick * 0.04;
  ctx.strokeStyle = "#94a3b8";
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.moveTo(r * 0.5, -r * 0.5);
  ctx.lineTo(r * 0.5 + Math.cos(dishAngle) * (r * 0.25), -r * 0.5 + Math.sin(dishAngle) * (r * 0.25));
  ctx.stroke();

  // Blinking red/green beacon light on the antenna tip
  const blink = (tick % 20 < 10);
  ctx.fillStyle = blink ? "#ef4444" : "#22c55e";
  ctx.beginPath();
  ctx.arc(r * 0.5 + Math.cos(dishAngle) * (r * 0.25), -r * 0.5 + Math.sin(dishAngle) * (r * 0.25), 1.5, 0, Math.PI * 2);
  ctx.fill();
}

/** Refinery: Industrial smelting complex with ore hopper, furnace vat & twin smokestacks */
function drawRefinery(
  ctx: CanvasRenderingContext2D,
  z: number,
  pal: TeamPalette,
  tick: number,
): void {
  const r = z * 0.4;

  // Drop shadow
  ctx.fillStyle = "rgba(0, 0, 0, 0.45)";
  ctx.fillRect(-r, -r + 2, r * 2, r * 2);

  // Main factory foundation
  ctx.fillStyle = "#1e293b";
  ctx.fillRect(-r, -r, r * 2, r * 2);
  ctx.strokeStyle = "#334155";
  ctx.lineWidth = 1;
  ctx.strokeRect(-r, -r, r * 2, r * 2);

  // Twin smokestacks at the rear
  const stackW = r * 0.32;
  const stackH = r * 0.55;
  for (const sx of [-r * 0.6, r * 0.28]) {
    // Pipe body
    ctx.fillStyle = "#0f172a";
    ctx.fillRect(sx, -r * 0.95, stackW, stackH);
    ctx.strokeStyle = "#475569";
    ctx.lineWidth = 1;
    ctx.strokeRect(sx, -r * 0.95, stackW, stackH);

    // Rim
    ctx.fillStyle = "#ea580c";
    ctx.fillRect(sx - 1, -r * 0.98, stackW + 2, 2.5);

    // Animated smoke puffs drifting upwards
    const puffOff = ((tick * 0.8 + (sx > 0 ? 5 : 0)) % 24);
    const puffAlpha = Math.max(0, 0.6 - puffOff / 24);
    ctx.fillStyle = `rgba(203, 213, 225, ${puffAlpha})`;
    ctx.beginPath();
    ctx.arc(sx + stackW / 2 + (puffOff * 0.2), -r * 0.98 - puffOff * 0.6, 2 + puffOff * 0.15, 0, Math.PI * 2);
    ctx.fill();
  }

  // Molten ore vat / smelting core
  const vatY = -r * 0.1;
  const vatW = r * 1.3;
  const vatH = r * 0.55;
  ctx.fillStyle = "#020617";
  ctx.fillRect(-vatW / 2, vatY, vatW, vatH);

  // Glowing molten gold interior
  const heatPulse = 0.7 + Math.sin(tick * 0.2) * 0.3;
  ctx.fillStyle = `rgba(234, 179, 8, ${heatPulse})`;
  ctx.fillRect(-vatW / 2 + 2, vatY + 2, vatW - 4, vatH - 4);

  // Slag grating bars over vat
  ctx.fillStyle = "#451a03";
  for (let i = -vatW / 2 + 4; i < vatW / 2 - 2; i += 3) {
    ctx.fillRect(i, vatY + 1, 1, vatH - 2);
  }

  // Front ore intake hopper ramp
  ctx.fillStyle = pal.primary;
  ctx.beginPath();
  ctx.moveTo(-r * 0.6, r * 0.5);
  ctx.lineTo(r * 0.6, r * 0.5);
  ctx.lineTo(r * 0.4, r * 0.95);
  ctx.lineTo(-r * 0.4, r * 0.95);
  ctx.closePath();
  ctx.fill();
  ctx.strokeStyle = pal.primaryLight;
  ctx.lineWidth = 1;
  ctx.stroke();

  // Team side panels
  ctx.fillStyle = pal.primaryDark;
  ctx.fillRect(-r * 0.95, -r * 0.2, r * 0.22, r * 0.8);
  ctx.fillRect(r * 0.73, -r * 0.2, r * 0.22, r * 0.8);
}

/** Barracks: Fortified troop compound with chevron blast roof & double blast doors */
function drawBarracks(
  ctx: CanvasRenderingContext2D,
  z: number,
  pal: TeamPalette,
  _tick: number,
): void {
  const r = z * 0.4;

  // Drop shadow
  ctx.fillStyle = "rgba(0, 0, 0, 0.45)";
  ctx.fillRect(-r, -r + 2, r * 2, r * 2);

  // Main compound body
  ctx.fillStyle = "#1e293b";
  ctx.fillRect(-r, -r, r * 2, r * 2);
  ctx.strokeStyle = "#334155";
  ctx.lineWidth = 1;
  ctx.strokeRect(-r, -r, r * 2, r * 2);

  // Sloped blast roof with team tactical chevrons
  ctx.fillStyle = pal.primary;
  ctx.beginPath();
  ctx.moveTo(-r * 0.85, -r * 0.85);
  ctx.lineTo(0, -r * 0.4);
  ctx.lineTo(r * 0.85, -r * 0.85);
  ctx.lineTo(r * 0.85, -r * 0.2);
  ctx.lineTo(0, r * 0.25);
  ctx.lineTo(-r * 0.85, -r * 0.2);
  ctx.closePath();
  ctx.fill();

  ctx.fillStyle = pal.primaryLight;
  ctx.beginPath();
  ctx.moveTo(-r * 0.7, -r * 0.7);
  ctx.lineTo(0, -r * 0.3);
  ctx.lineTo(r * 0.7, -r * 0.7);
  ctx.lineTo(r * 0.7, -r * 0.55);
  ctx.lineTo(0, -r * 0.15);
  ctx.lineTo(-r * 0.7, -r * 0.55);
  ctx.closePath();
  ctx.fill();

  // Double hydraulic blast doors
  const doorW = r * 0.7;
  const doorH = r * 0.5;
  const doorY = r * 0.4;
  ctx.fillStyle = "#0f172a";
  ctx.fillRect(-doorW / 2, doorY, doorW, doorH);
  ctx.strokeStyle = "#475569";
  ctx.lineWidth = 1;
  ctx.strokeRect(-doorW / 2, doorY, doorW, doorH);

  // Center door seam & warning lights
  ctx.strokeStyle = "#64748b";
  ctx.beginPath();
  ctx.moveTo(0, doorY);
  ctx.lineTo(0, doorY + doorH);
  ctx.stroke();

  // Status lights (green ready lights)
  ctx.fillStyle = "#22c55e";
  ctx.fillRect(-doorW / 2 + 1.5, doorY + 2, 2, 2);
  ctx.fillRect(doorW / 2 - 3.5, doorY + 2, 2, 2);

  // Ventilation grilles on the sides
  ctx.fillStyle = "#0f172a";
  ctx.fillRect(-r * 0.88, r * 0.45, r * 0.18, r * 0.4);
  ctx.fillRect(r * 0.7, r * 0.45, r * 0.18, r * 0.4);
}

/** Factory: Mechanized vehicle foundry with hazard-striped roll-up bay door & crane rails */
function drawFactory(
  ctx: CanvasRenderingContext2D,
  z: number,
  pal: TeamPalette,
  _tick: number,
): void {
  const r = z * 0.4;

  // Drop shadow
  ctx.fillStyle = "rgba(0, 0, 0, 0.45)";
  ctx.fillRect(-r, -r + 2, r * 2, r * 2);

  // Heavy steel foundry body
  ctx.fillStyle = "#1e242c";
  ctx.fillRect(-r, -r, r * 2, r * 2);
  ctx.strokeStyle = "#333d4b";
  ctx.lineWidth = 1.5;
  ctx.strokeRect(-r, -r, r * 2, r * 2);

  // Overhead crane gantry across roof
  ctx.fillStyle = "#334155";
  ctx.fillRect(-r * 0.9, -r * 0.85, r * 1.8, r * 0.3);
  ctx.strokeStyle = "#64748b";
  ctx.lineWidth = 0.75;
  ctx.strokeRect(-r * 0.9, -r * 0.85, r * 1.8, r * 0.3);

  // Team identification bar
  ctx.fillStyle = pal.primary;
  ctx.fillRect(-r * 0.85, -r * 0.45, r * 1.7, r * 0.2);

  // Large roll-up hangar door
  const bayW = r * 1.35;
  const bayH = r * 0.85;
  const bayY = -r * 0.15;
  ctx.fillStyle = "#090d16";
  ctx.fillRect(-bayW / 2, bayY, bayW, bayH);

  // Segmented roll-up horizontal slats
  ctx.strokeStyle = "#1e293b";
  ctx.lineWidth = 1;
  for (let y = bayY + 3; y < bayY + bayH; y += 3.5) {
    ctx.beginPath();
    ctx.moveTo(-bayW / 2 + 1, y);
    ctx.lineTo(bayW / 2 - 1, y);
    ctx.stroke();
  }

  // Yellow/black diagonal hazard stripes on the door threshold
  const stripeH = Math.max(2.5, r * 0.18);
  const stripeY = bayY + bayH - stripeH;
  ctx.fillStyle = "#eab308";
  ctx.fillRect(-bayW / 2, stripeY, bayW, stripeH);

  ctx.fillStyle = "#18181b";
  ctx.beginPath();
  for (let x = -bayW / 2; x < bayW / 2 + stripeH; x += stripeH * 1.2) {
    ctx.moveTo(x, stripeY + stripeH);
    ctx.lineTo(x + stripeH * 0.6, stripeY + stripeH);
    ctx.lineTo(x + stripeH * 1.1, stripeY);
    ctx.lineTo(x + stripeH * 0.5, stripeY);
  }
  ctx.closePath();
  ctx.fill();

  // Heavy hydraulic side stabilizer columns
  ctx.fillStyle = "#475569";
  ctx.fillRect(-r * 0.98, bayY, r * 0.15, bayH);
  ctx.fillRect(r * 0.83, bayY, r * 0.15, bayH);
}

/** TechLab: Geodesic research dome with pulsing plasma core & satellite uplink */
function drawTechLab(
  ctx: CanvasRenderingContext2D,
  z: number,
  pal: TeamPalette,
  tick: number,
): void {
  const r = z * 0.4;

  // Drop shadow
  ctx.fillStyle = "rgba(0, 0, 0, 0.45)";
  ctx.beginPath();
  ctx.arc(0, 2, r, 0, Math.PI * 2);
  ctx.fill();

  // Hexagonal composite base plate
  ctx.fillStyle = "#111827";
  ctx.strokeStyle = "#374151";
  ctx.lineWidth = 1.5;
  drawRegularPolygon(ctx, 0, 0, r, 6);
  ctx.fill();
  ctx.stroke();

  // Team energy conduit traces
  ctx.strokeStyle = pal.primary;
  ctx.lineWidth = 1.5;
  for (let i = 0; i < 6; i++) {
    const angle = (i * Math.PI) / 3;
    ctx.beginPath();
    ctx.moveTo(0, 0);
    ctx.lineTo(Math.cos(angle) * r * 0.85, Math.sin(angle) * r * 0.85);
    ctx.stroke();
  }

  // Geodesic containment dome
  const domeR = r * 0.6;
  ctx.fillStyle = "#1e293b";
  ctx.beginPath();
  ctx.arc(0, 0, domeR, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = "#64748b";
  ctx.lineWidth = 1;
  ctx.stroke();

  // Pulsing plasma sphere
  const pulse = 0.6 + Math.sin(tick * 0.2) * 0.4;
  const plasmaGrad = ctx.createRadialGradient(0, 0, 0, 0, 0, domeR * 0.7);
  plasmaGrad.addColorStop(0, "#ffffff");
  plasmaGrad.addColorStop(0.3, pal.accent);
  plasmaGrad.addColorStop(0.7, pal.primary);
  plasmaGrad.addColorStop(1, "rgba(30, 41, 59, 0)");
  ctx.fillStyle = plasmaGrad;
  ctx.beginPath();
  ctx.arc(0, 0, domeR * (0.6 + pulse * 0.15), 0, Math.PI * 2);
  ctx.fill();

  // Orbiting containment ring / satellite sensor
  const orbitAngle = tick * 0.08;
  const ox = Math.cos(orbitAngle) * domeR * 0.85;
  const oy = Math.sin(orbitAngle) * domeR * 0.85;
  ctx.fillStyle = "#38bdf8";
  ctx.beginPath();
  ctx.arc(ox, oy, 2, 0, Math.PI * 2);
  ctx.fill();
}

/** Turret: Heavy point defense circular emplacement with twin auto-cannon barrels */
function drawTurret(
  ctx: CanvasRenderingContext2D,
  z: number,
  pal: TeamPalette,
  _tick: number,
): void {
  const r = z * 0.38;

  // Drop shadow
  ctx.fillStyle = "rgba(0, 0, 0, 0.4)";
  ctx.beginPath();
  ctx.arc(0, 2, r, 0, Math.PI * 2);
  ctx.fill();

  // Base armored ring with mounting bolts
  ctx.fillStyle = "#1e242c";
  ctx.beginPath();
  ctx.arc(0, 0, r, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = "#334155";
  ctx.lineWidth = 1.5;
  ctx.stroke();

  // 6 mounting bolts
  ctx.fillStyle = "#64748b";
  for (let i = 0; i < 6; i++) {
    const angle = (i * Math.PI) / 3;
    const bx = Math.cos(angle) * r * 0.78;
    const by = Math.sin(angle) * r * 0.78;
    ctx.fillRect(bx - 1, by - 1, 2, 2);
  }

  // Twin auto-cannon barrels pointing forward (default facing up/right)
  const barrelLen = r * 1.1;
  const barrelW = Math.max(1.5, r * 0.22);
  const barrelSpacing = r * 0.28;

  ctx.fillStyle = "#0f172a";
  ctx.strokeStyle = "#475569";
  ctx.lineWidth = 0.75;

  // Left barrel
  ctx.fillRect(-barrelSpacing - barrelW / 2, -barrelLen, barrelW, barrelLen);
  ctx.strokeRect(-barrelSpacing - barrelW / 2, -barrelLen, barrelW, barrelLen);

  // Right barrel
  ctx.fillRect(barrelSpacing - barrelW / 2, -barrelLen, barrelW, barrelLen);
  ctx.strokeRect(barrelSpacing - barrelW / 2, -barrelLen, barrelW, barrelLen);

  // Muzzle flash arrestors / tips
  ctx.fillStyle = "#334155";
  ctx.fillRect(-barrelSpacing - barrelW / 2 - 0.5, -barrelLen, barrelW + 1, 2);
  ctx.fillRect(barrelSpacing - barrelW / 2 - 0.5, -barrelLen, barrelW + 1, 2);

  // Rotating center cupola
  const cupolaR = r * 0.55;
  ctx.fillStyle = pal.primary;
  ctx.beginPath();
  ctx.arc(0, 0, cupolaR, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = pal.primaryLight;
  ctx.lineWidth = 1;
  ctx.stroke();

  // Central viewport / laser sensor optic
  ctx.fillStyle = "#ef4444";
  ctx.fillRect(-1.5, -cupolaR * 0.6, 3, 2);
}

// ---------------------------------------------------------------------------
// Unit Sprites
// ---------------------------------------------------------------------------

export function drawUnitSprite(
  ctx: CanvasRenderingContext2D,
  kind: string,
  px: number,
  py: number,
  zoom: number,
  owner: number,
  heading: number,
  tick: number,
  isStale: boolean = false,
  carryingOre: number = 0,
): void {
  const pal = getTeamPalette(owner, isStale);

  ctx.save();
  ctx.translate(px, py);
  ctx.rotate(heading);

  switch (kind) {
    case "Infantry":
      drawInfantry(ctx, zoom, pal, tick);
      break;
    case "Tank":
      drawTank(ctx, zoom, pal, tick);
      break;
    case "Artillery":
      drawArtillery(ctx, zoom, pal, tick);
      break;
    case "Harvester":
      drawHarvester(ctx, zoom, pal, tick, carryingOre);
      break;
    default:
      ctx.fillStyle = pal.primary;
      ctx.beginPath();
      ctx.arc(0, 0, zoom * 0.25, 0, Math.PI * 2);
      ctx.fill();
  }

  ctx.restore();
}

/** Infantry: Armored cybernetic trooper with shoulder armor, helmet visor & rifle */
function drawInfantry(
  ctx: CanvasRenderingContext2D,
  z: number,
  pal: TeamPalette,
  _tick: number,
): void {
  const s = z * 0.32;

  // Drop shadow
  ctx.fillStyle = "rgba(0, 0, 0, 0.4)";
  ctx.beginPath();
  ctx.ellipse(0, 1, s * 0.7, s * 0.5, 0, 0, Math.PI * 2);
  ctx.fill();

  // Assault rifle barrel extending forward (+X is forward)
  ctx.fillStyle = "#0f172a";
  ctx.fillRect(s * 0.2, s * 0.15, s * 0.95, Math.max(1.5, s * 0.22));
  ctx.fillStyle = "#475569";
  ctx.fillRect(s * 0.8, s * 0.12, s * 0.35, Math.max(1, s * 0.12));

  // Armored torso / plate carrier
  ctx.fillStyle = pal.primary;
  ctx.beginPath();
  drawRoundedRect(ctx, -s * 0.55, -s * 0.45, s * 0.9, s * 0.9, s * 0.2);
  ctx.fill();
  ctx.strokeStyle = pal.primaryDark;
  ctx.lineWidth = 1;
  ctx.stroke();

  // Shoulder pauldrons
  ctx.fillStyle = pal.primaryLight;
  ctx.fillRect(-s * 0.4, -s * 0.65, s * 0.45, s * 0.25);
  ctx.fillRect(-s * 0.4, s * 0.4, s * 0.45, s * 0.25);

  // Armored helmet
  const headR = s * 0.35;
  ctx.fillStyle = "#1e293b";
  ctx.beginPath();
  ctx.arc(-s * 0.1, 0, headR, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = "#475569";
  ctx.lineWidth = 0.75;
  ctx.stroke();

  // Visor glowing slit facing forward (+X)
  ctx.fillStyle = pal.accent;
  ctx.fillRect(s * 0.1, -headR * 0.45, headR * 0.4, headR * 0.9);
}

/** Tank: Heavy battle tank with dual track treads, sloped armor & rotating cannon turret */
function drawTank(
  ctx: CanvasRenderingContext2D,
  z: number,
  pal: TeamPalette,
  _tick: number,
): void {
  const s = z * 0.38;

  // Drop shadow
  ctx.fillStyle = "rgba(0, 0, 0, 0.45)";
  ctx.fillRect(-s * 0.9, -s * 0.7 + 2, s * 1.8, s * 1.4);

  // Left & right track treads (+X is forward)
  const treadW = s * 1.7;
  const treadH = s * 0.35;
  const treadY = s * 0.5;

  ctx.fillStyle = "#0f172a";
  ctx.fillRect(-treadW / 2, -treadY - treadH / 2, treadW, treadH);
  ctx.fillRect(-treadW / 2, treadY - treadH / 2, treadW, treadH);

  // Tread links / sprockets
  ctx.strokeStyle = "#334155";
  ctx.lineWidth = 1;
  for (let x = -treadW / 2 + 2; x < treadW / 2; x += 3.5) {
    ctx.beginPath();
    ctx.moveTo(x, -treadY - treadH / 2);
    ctx.lineTo(x, -treadY + treadH / 2);
    ctx.moveTo(x, treadY - treadH / 2);
    ctx.lineTo(x, treadY + treadH / 2);
    ctx.stroke();
  }

  // Sloped armored hull chassis
  const hullW = s * 1.35;
  const hullH = s * 0.85;
  ctx.fillStyle = "#1e293b";
  ctx.beginPath();
  drawRoundedRect(ctx, -hullW / 2, -hullH / 2, hullW, hullH, s * 0.15);
  ctx.fill();
  ctx.strokeStyle = "#334155";
  ctx.lineWidth = 1;
  ctx.stroke();

  // Team colored hull deck plate
  ctx.fillStyle = pal.primary;
  ctx.fillRect(-hullW * 0.35, -hullH * 0.4, hullW * 0.7, hullH * 0.8);

  // Engine exhaust grilles at the rear (-X)
  ctx.fillStyle = "#020617";
  ctx.fillRect(-hullW / 2 + 2, -hullH * 0.3, 3, hullH * 0.6);

  // Main cannon barrel extending forward (+X)
  const barrelLen = s * 1.25;
  const barrelW = Math.max(1.5, s * 0.22);
  ctx.fillStyle = "#0f172a";
  ctx.fillRect(0, -barrelW / 2, barrelLen, barrelW);
  ctx.strokeStyle = "#475569";
  ctx.lineWidth = 0.75;
  ctx.strokeRect(0, -barrelW / 2, barrelLen, barrelW);

  // Muzzle brake & bore evacuator
  ctx.fillStyle = "#334155";
  ctx.fillRect(barrelLen * 0.5, -barrelW * 0.7, barrelW * 1.2, barrelW * 1.4);
  ctx.fillRect(barrelLen - 2, -barrelW * 0.8, 3, barrelW * 1.6);

  // Armored turret cupola
  const turretR = s * 0.45;
  ctx.fillStyle = pal.primaryDark;
  ctx.beginPath();
  ctx.arc(0, 0, turretR, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = pal.primaryLight;
  ctx.lineWidth = 1;
  ctx.stroke();

  // Commander hatch
  ctx.fillStyle = "#0f172a";
  ctx.beginPath();
  ctx.arc(-turretR * 0.2, 0, turretR * 0.35, 0, Math.PI * 2);
  ctx.fill();
}

/** Artillery: Heavy crawler with outrigger stabilizers & extended railgun/howitzer barrel */
function drawArtillery(
  ctx: CanvasRenderingContext2D,
  z: number,
  pal: TeamPalette,
  _tick: number,
): void {
  const s = z * 0.4;

  // Drop shadow
  ctx.fillStyle = "rgba(0, 0, 0, 0.45)";
  ctx.fillRect(-s * 0.9, -s * 0.8 + 2, s * 1.8, s * 1.6);

  // Outrigger wide crawler tracks
  const trackW = s * 1.6;
  const trackH = s * 0.4;
  const trackY = s * 0.65;

  ctx.fillStyle = "#0f172a";
  ctx.fillRect(-trackW / 2, -trackY - trackH / 2, trackW, trackH);
  ctx.fillRect(-trackW / 2, trackY - trackH / 2, trackW, trackH);

  // Stabilizer arm struts
  ctx.fillStyle = "#334155";
  ctx.fillRect(-s * 0.6, -trackY + trackH / 2, s * 1.2, trackY * 2 - trackH);

  // Main chassis
  const bodyW = s * 1.1;
  const bodyH = s * 0.85;
  ctx.fillStyle = "#1e293b";
  ctx.fillRect(-bodyW / 2, -bodyH / 2, bodyW, bodyH);
  ctx.strokeStyle = "#475569";
  ctx.lineWidth = 1;
  ctx.strokeRect(-bodyW / 2, -bodyH / 2, bodyW, bodyH);

  // Team rear armored plate
  ctx.fillStyle = pal.primary;
  ctx.fillRect(-bodyW / 2, -bodyH / 2, bodyW * 0.45, bodyH);

  // Long-range siege railgun / heavy howitzer cannon (+X is forward)
  const barrelLen = s * 1.7;
  const barrelW = Math.max(2, s * 0.28);

  // Recoil compensation housing
  ctx.fillStyle = "#0f172a";
  ctx.fillRect(-s * 0.2, -barrelW * 0.8, s * 0.7, barrelW * 1.6);

  // Heavy cannon barrel
  ctx.fillStyle = "#020617";
  ctx.fillRect(0, -barrelW / 2, barrelLen, barrelW);
  ctx.strokeStyle = "#64748b";
  ctx.lineWidth = 1;
  ctx.strokeRect(0, -barrelW / 2, barrelLen, barrelW);

  // Railgun energy coil bands along barrel
  ctx.fillStyle = pal.accent;
  for (let x = s * 0.5; x < barrelLen - 4; x += s * 0.35) {
    ctx.fillRect(x, -barrelW * 0.7, 2.5, barrelW * 1.4);
  }

  // Heavy muzzle brake
  ctx.fillStyle = "#475569";
  ctx.fillRect(barrelLen - 3, -barrelW * 0.9, 4, barrelW * 1.8);
}

/** Harvester: Heavy 6-wheel rover with front drill/collector & rear ore cargo bed */
function drawHarvester(
  ctx: CanvasRenderingContext2D,
  z: number,
  pal: TeamPalette,
  tick: number,
  carryingOre: number,
): void {
  const s = z * 0.36;

  // Drop shadow
  ctx.fillStyle = "rgba(0, 0, 0, 0.45)";
  ctx.fillRect(-s * 0.9, -s * 0.7 + 2, s * 1.8, s * 1.4);

  // 6 heavy all-terrain wheels (+X is forward)
  const wheelW = s * 0.42;
  const wheelH = s * 0.28;
  const wheelY = s * 0.55;

  ctx.fillStyle = "#0f172a";
  for (const x of [-s * 0.6, 0, s * 0.6]) {
    ctx.fillRect(x - wheelW / 2, -wheelY - wheelH / 2, wheelW, wheelH);
    ctx.fillRect(x - wheelW / 2, wheelY - wheelH / 2, wheelW, wheelH);
    // Rim hubs
    ctx.fillStyle = "#475569";
    ctx.fillRect(x - 1, -wheelY - 1, 2, 2);
    ctx.fillRect(x - 1, wheelY - 1, 2, 2);
    ctx.fillStyle = "#0f172a";
  }

  // Heavy rover chassis
  const chassisW = s * 1.4;
  const chassisH = s * 0.85;
  ctx.fillStyle = "#1e293b";
  ctx.fillRect(-chassisW / 2, -chassisH / 2, chassisW, chassisH);
  ctx.strokeStyle = "#334155";
  ctx.lineWidth = 1;
  ctx.strokeRect(-chassisW / 2, -chassisH / 2, chassisW, chassisH);

  // Rear cargo bed (-X)
  const bedW = chassisW * 0.55;
  const bedH = chassisH * 0.8;
  const bedX = -chassisW / 2 + 2;
  const bedY = -bedH / 2;

  ctx.fillStyle = "#090d16";
  ctx.fillRect(bedX, bedY, bedW, bedH);
  ctx.strokeStyle = "#475569";
  ctx.lineWidth = 0.75;
  ctx.strokeRect(bedX, bedY, bedW, bedH);

  // If carrying ore: show glowing gold crystals inside cargo bed!
  if (carryingOre > 0) {
    ctx.fillStyle = "#eab308";
    const oreCount = Math.min(6, Math.max(2, Math.floor(carryingOre / 8)));
    for (let i = 0; i < oreCount; i++) {
      const ox = bedX + 2 + (i % 3) * (bedW / 3.2);
      const oy = bedY + 2 + Math.floor(i / 3) * (bedH / 2.2);
      ctx.fillRect(ox, oy, bedW / 4, bedH / 3);
    }
    // Specular highlight on ore load
    ctx.fillStyle = "#fef08a";
    ctx.fillRect(bedX + 3, bedY + 3, 2, 2);
  }

  // Front operator cabin (+X)
  const cabW = chassisW * 0.35;
  const cabH = chassisH * 0.75;
  const cabX = chassisW / 2 - cabW - 2;
  const cabY = -cabH / 2;

  ctx.fillStyle = pal.primary;
  ctx.fillRect(cabX, cabY, cabW, cabH);
  ctx.strokeStyle = pal.primaryDark;
  ctx.lineWidth = 1;
  ctx.strokeRect(cabX, cabY, cabW, cabH);

  // Protective cockpit windshield
  ctx.fillStyle = "#38bdf8";
  ctx.fillRect(cabX + cabW * 0.4, cabY + 2, cabW * 0.5, cabH - 4);

  // Front articulating mining drill / laser intake arms extending forward
  const armLen = s * 0.55;
  const drillSpin = tick * 0.3;
  ctx.fillStyle = "#475569";
  ctx.fillRect(chassisW / 2 - 1, -s * 0.35, armLen, s * 0.18);
  ctx.fillRect(chassisW / 2 - 1, s * 0.17, armLen, s * 0.18);

  // Rotary cutting drill head
  ctx.fillStyle = "#e2e8f0";
  ctx.beginPath();
  ctx.arc(chassisW / 2 + armLen, -s * 0.26 + Math.sin(drillSpin) * 1, 2.5, 0, Math.PI * 2);
  ctx.arc(chassisW / 2 + armLen, s * 0.26 - Math.sin(drillSpin) * 1, 2.5, 0, Math.PI * 2);
  ctx.fill();
}

// ---------------------------------------------------------------------------
// FX & HUD Elements
// ---------------------------------------------------------------------------

/** Sci-fi 4-corner targeting bracket reticle */
export function drawSelectionReticle(
  ctx: CanvasRenderingContext2D,
  px: number,
  py: number,
  size: number,
  tick: number,
): void {
  const half = size * 0.55;
  const arm = Math.max(3, size * 0.22);
  const pulse = 0.85 + Math.sin(tick * 0.12) * 0.15;

  ctx.save();
  ctx.translate(px, py);
  ctx.strokeStyle = `rgba(255, 226, 122, ${pulse})`;
  ctx.lineWidth = 1.75;

  // Top-left
  ctx.beginPath();
  ctx.moveTo(-half, -half + arm);
  ctx.lineTo(-half, -half);
  ctx.lineTo(-half + arm, -half);
  ctx.stroke();

  // Top-right
  ctx.beginPath();
  ctx.moveTo(half - arm, -half);
  ctx.lineTo(half, -half);
  ctx.lineTo(half, -half + arm);
  ctx.stroke();

  // Bottom-right
  ctx.beginPath();
  ctx.moveTo(half, half - arm);
  ctx.lineTo(half, half);
  ctx.lineTo(half - arm, half);
  ctx.stroke();

  // Bottom-left
  ctx.beginPath();
  ctx.moveTo(-half + arm, half);
  ctx.lineTo(-half, half);
  ctx.lineTo(-half, half - arm);
  ctx.stroke();

  ctx.restore();
}

/** Stylized Health Bar */
export function drawHealthBar(
  ctx: CanvasRenderingContext2D,
  px: number,
  py: number,
  size: number,
  hp: number,
  maxHp: number,
): void {
  if (maxHp <= 0) return;
  const frac = Math.max(0, Math.min(1, hp / maxHp));

  const barW = size * 0.85;
  const barH = Math.max(3, size * 0.1);
  const bx = px - barW / 2;
  const by = py - size * 0.65;

  // Dark background frame
  ctx.fillStyle = "rgba(10, 14, 18, 0.85)";
  ctx.fillRect(bx - 1, by - 1, barW + 2, barH + 2);
  ctx.strokeStyle = "rgba(51, 65, 85, 0.9)";
  ctx.lineWidth = 0.5;
  ctx.strokeRect(bx - 1, by - 1, barW + 2, barH + 2);

  // Gradient health bar
  const hpColor = frac > 0.5 ? "#22c55e" : frac > 0.25 ? "#eab308" : "#ef4444";
  ctx.fillStyle = hpColor;
  ctx.fillRect(bx, by, barW * frac, barH);

  // Subtle gloss highlight
  ctx.fillStyle = "rgba(255, 255, 255, 0.35)";
  ctx.fillRect(bx, by, barW * frac, barH * 0.4);
}

/** Building Rally Point Marker and Line */
export function drawRallyPoint(
  ctx: CanvasRenderingContext2D,
  bpx: number,
  bpy: number,
  rpx: number,
  rpy: number,
  tick: number,
): void {
  // Dashed rally path line
  ctx.save();
  ctx.strokeStyle = "rgba(255, 226, 122, 0.65)";
  ctx.lineWidth = 1.5;
  ctx.setLineDash([4, 4]);
  ctx.lineDashOffset = -(tick * 0.5) % 8;
  ctx.beginPath();
  ctx.moveTo(bpx, bpy);
  ctx.lineTo(rpx, rpy);
  ctx.stroke();

  // Target waypoint beacon / flag
  ctx.setLineDash([]);
  const pulse = 4 + Math.sin(tick * 0.15) * 2;
  ctx.strokeStyle = "#ffe27a";
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.arc(rpx, rpy, pulse, 0, Math.PI * 2);
  ctx.stroke();

  // Center beacon dot
  ctx.fillStyle = "#ffe27a";
  ctx.beginPath();
  ctx.arc(rpx, rpy, 2, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}

// ---------------------------------------------------------------------------
// Canvas Path Helpers
// ---------------------------------------------------------------------------

function drawRoundedRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.lineTo(x + w - r, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + r);
  ctx.lineTo(x + w, y + h - r);
  ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  ctx.lineTo(x + r, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - r);
  ctx.lineTo(x, y + r);
  ctx.quadraticCurveTo(x, y, x + r, y);
  ctx.closePath();
}

function drawChamferedRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  c: number,
): void {
  ctx.beginPath();
  ctx.moveTo(x + c, y);
  ctx.lineTo(x + w - c, y);
  ctx.lineTo(x + w, y + c);
  ctx.lineTo(x + w, y + h - c);
  ctx.lineTo(x + w - c, y + h);
  ctx.lineTo(x + c, y + h);
  ctx.lineTo(x, y + h - c);
  ctx.lineTo(x, y + c);
  ctx.closePath();
}

function drawRegularPolygon(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  radius: number,
  sides: number,
): void {
  ctx.beginPath();
  for (let i = 0; i < sides; i++) {
    const angle = (i * 2 * Math.PI) / sides;
    const x = cx + radius * Math.cos(angle);
    const y = cy + radius * Math.sin(angle);
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.closePath();
}
