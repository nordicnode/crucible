// Client entry point: lobby, match loop, input, and HUD. All game logic is
// server-side; this only renders fogged state and forwards commands.

import { initDashboard } from "./dashboard";
import { Net } from "./net";
import { drawRadar, Renderer } from "./renderer";
import { spectate } from "./spectate";
import { World } from "./world";
import {
  BUILDING_KINDS,
  BUILD_COSTS,
  UNIT_COSTS,
  UNIT_KINDS,
  chooseUpgrade,
  moveGroup,
  placeBuilding,
  sell,
  setRally,
  trainUnit,
  type BuildingType,
  type Command,
  type ServerMsg,
  type UnitType,
} from "./types";

const canvas = document.getElementById("view") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;

const net = new Net();
const world = new World();
const renderer = new Renderer();

let selection = new Set<number>();
let placementMode: BuildingType | null = null;
let placementCursor: [number, number] | null = null;
let opponentLabel = "hard";

// Input drag state.
let dragStart: [number, number] | null = null;
let dragCurrent: [number, number] | null = null;
let panning = false;
let lastPan: [number, number] | null = null;

function el<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

// ---------------------------------------------------------------------------
// Server messages
// ---------------------------------------------------------------------------

function onServerMsg(msg: ServerMsg): void {
  switch (msg.type) {
    case "matchStart": {
      world.setMap(msg.mapSeed, msg.passable, msg.hq);
      const ownHq = msg.hq[msg.player];
      renderer.camera.focusOn(ownHq[0] + 0.5, ownHq[1] + 0.5, 18, canvas.width, canvas.height);
      el("overlay").classList.add("hidden");
      el("sidebar").classList.remove("hidden");
      el("topbar").classList.remove("hidden");
      el("log").classList.remove("hidden");
      el("opponent").textContent = opponentLabel;
      break;
    }
    case "stateDiff": {
      world.applyDiff(msg.tick, msg.ore, msg.entities, msg.oreTiles, msg.visible, msg.events);
      break;
    }
    case "matchEnd": {
      world.result = { winner: msg.winner, reason: msg.reason };
      const win = msg.winner === 0;
      el("result-title").textContent = win ? "VICTORY" : "DEFEAT";
      el("result-title").className = win ? "win" : "lose";
      el("result-detail").textContent =
        `${msg.reason} · ${formatClock(msg.durationTicks)} · replay #${msg.replayId ?? "?"}`;
      el("overlay").classList.remove("hidden");
      el("lobby").classList.add("hidden");
      el("result").classList.remove("hidden");
      break;
    }
  }
}

function startMatch(which: string, label?: string): void {
  opponentLabel = label ?? which;
  selection = new Set();
  placementMode = null;
  net.close();
  net.connect(onServerMsg, showLobby);
  net.send({ type: "joinMatch", opponent: which });
}

function showLobby(): void {
  el("overlay").classList.remove("hidden");
  el("lobby").classList.remove("hidden");
  el("result").classList.add("hidden");
  el("sidebar").classList.add("hidden");
  el("topbar").classList.add("hidden");
  el("log").classList.add("hidden");
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

function sendCommands(cmds: Command[]): void {
  if (cmds.length > 0) net.send({ type: "commands", cmds });
}

function selectedUnits(): number[] {
  return [...selection].filter((id) => {
    const e = world.entities.get(id);
    return e && e.owner === 0 && UNIT_KINDS.has(e.kind);
  });
}

function selectedSingle(): number | null {
  return selection.size === 1 ? [...selection][0] : null;
}

function issueMove(tile: [number, number]): void {
  const units = selectedUnits();
  if (units.length > 0) {
    sendCommands([moveGroup(units, tile)]);
    return;
  }
  const single = selectedSingle();
  if (single != null) {
    const e = world.entities.get(single);
    if (e && BUILDING_KINDS.has(e.kind)) {
      sendCommands([setRally(single, tile)]);
    }
  }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

function canvasPos(ev: MouseEvent): [number, number] {
  const r = canvas.getBoundingClientRect();
  return [ev.clientX - r.left, ev.clientY - r.top];
}

function tileAt(sx: number, sy: number): [number, number] {
  return [Math.floor(renderer.camera.worldX(sx)), Math.floor(renderer.camera.worldY(sy))];
}

canvas.addEventListener("mousedown", (ev) => {
  const [sx, sy] = canvasPos(ev);
  if (spectate.active) {
    if (ev.button === 1) {
      panning = true;
      lastPan = [sx, sy];
    }
    return;
  }
  if (ev.button === 0) {
    if (placementMode) {
      sendCommands([placeBuilding(placementMode, tileAt(sx, sy))]);
      placementMode = null;
      placementCursor = null;
    } else {
      dragStart = [sx, sy];
      dragCurrent = [sx, sy];
      if (!ev.shiftKey) selection = new Set();
    }
  } else if (ev.button === 1) {
    panning = true;
    lastPan = [sx, sy];
  } else if (ev.button === 2) {
    placementMode = null;
    issueMove(tileAt(sx, sy));
  }
});

canvas.addEventListener("mousemove", (ev) => {
  const [sx, sy] = canvasPos(ev);
  if (panning && lastPan) {
    const cam = spectate.active ? spectate.renderer.camera : renderer.camera;
    cam.pan(sx - lastPan[0], sy - lastPan[1]);
    lastPan = [sx, sy];
  } else if (dragStart) {
    dragCurrent = [sx, sy];
  }
  if (!spectate.active && placementMode && !panning && !dragStart) {
    placementCursor = tileAt(sx, sy);
  }
});

canvas.addEventListener("mouseup", (ev) => {
  if (ev.button === 1) {
    panning = false;
    lastPan = null;
  }
  if (spectate.active || ev.button !== 0 || !dragStart) return;
  const start = dragStart;
  const [sx, sy] = canvasPos(ev);
  dragStart = null;
  dragCurrent = null;
  if (Math.hypot(sx - start[0], sy - start[1]) < 4) {
    selectAt(sx, sy, ev.shiftKey);
  } else {
    boxSelect(start, [sx, sy]);
  }
});

function boxSelect(a: [number, number], b: [number, number]): void {
  const minX = Math.min(a[0], b[0]), maxX = Math.max(a[0], b[0]);
  const minY = Math.min(a[1], b[1]), maxY = Math.max(a[1], b[1]);
  for (const e of world.ownUnits) {
    const sx = renderer.camera.screenX(e.x);
    const sy = renderer.camera.screenY(e.y);
    if (sx >= minX && sx <= maxX && sy >= minY && sy <= maxY) selection.add(e.id);
  }
}

function selectAt(sx: number, sy: number, additive: boolean): void {
  const [tx, ty] = tileAt(sx, sy);
  let bestId: number | null = null;
  let bestDist = Infinity;
  for (const e of world.entities.values()) {
    if (e.owner !== 0) continue;
    const dx = e.x - (tx + 0.5);
    const dy = e.y - (ty + 0.5);
    const d = dx * dx + dy * dy;
    if (d < 0.4 && d < bestDist) {
      bestDist = d;
      bestId = e.id;
    }
  }
  if (bestId != null) {
    if (!additive) selection = new Set();
    selection.add(bestId);
  }
}

canvas.addEventListener("wheel", (ev) => {
  ev.preventDefault();
  const [sx, sy] = canvasPos(ev);
  const cam = spectate.active ? spectate.renderer.camera : renderer.camera;
  cam.zoomAt(sx, sy, ev.deltaY < 0 ? 1.15 : 1 / 1.15);
});

canvas.addEventListener("contextmenu", (ev) => ev.preventDefault());

window.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape") {
    placementMode = null;
    placementCursor = null;
  }
});

// ---------------------------------------------------------------------------
// HUD — C&C-style command sidebar
// ---------------------------------------------------------------------------

let lastPanelSig = "";

/// Icon + label for build/unit buttons.
const BUTTON_META: Record<string, { ic: string; label: string }> = {
  Refinery: { ic: "⛏", label: "Refinery" },
  Barracks: { ic: "🪖", label: "Barracks" },
  Factory: { ic: "🏭", label: "Factory" },
  TechLab: { ic: "🔬", label: "Tech Lab" },
  Turret: { ic: "🛡", label: "Turret" },
  Harvester: { ic: "⛏", label: "Harvester" },
  Infantry: { ic: "🪖", label: "Infantry" },
  Tank: { ic: "🚜", label: "Tank" },
  Artillery: { ic: "💥", label: "Artillery" },
};

function cmdButton(
  key: string,
  cost: number,
  onClick: () => void,
  opts: { armed?: boolean } = {},
): HTMLButtonElement {
  const meta = BUTTON_META[key] ?? { ic: "▣", label: key };
  const b = document.createElement("button");
  b.className = "cmd";
  b.innerHTML = `<span class="ic">${meta.ic}</span>${meta.label}`;
  if (cost > 0) {
    const c = document.createElement("span");
    c.className = "cost";
    c.textContent = String(cost);
    b.appendChild(c);
    if (world.ore < cost) b.classList.add("disabled");
  }
  if (opts.armed) b.classList.add("armed");
  if (!b.classList.contains("disabled")) b.addEventListener("click", onClick);
  return b;
}

function producibleUnits(kind: string): UnitType[] {
  if (kind === "Barracks") return ["Infantry"];
  if (kind === "Factory") {
    const hasLab = world.ownBuildings.some((b) => b.kind === "TechLab");
    return hasLab ? ["Harvester", "Tank", "Artillery"] : ["Harvester", "Tank"];
  }
  return [];
}

function renderCommandSidebar(): void {
  const single = selectedSingle();
  const selEntity = single != null ? world.entities.get(single) : null;
  const qsig = selEntity && selEntity.queue ? `${selEntity.progress}/${selEntity.buildTime}` : "";
  const sig =
    [...selection].sort((a, b) => a - b).join(",") + "|" + placementMode + "|" + qsig + "|" + world.ore;
  if (sig === lastPanelSig) return;
  lastPanelSig = sig;

  // Selection card.
  const name = el("sel-name");
  const detail = el("sel-detail");
  const hpwrap = el("sel-hpwrap");
  const hp = el("sel-hp");
  const queue = el("sel-queue");

  if (selEntity) {
    name.textContent = selEntity.kind;
    const hpText = selEntity.maxHp > 0 ? `${selEntity.hp}/${selEntity.maxHp}` : "";
    detail.textContent = hpText + (selectedUnits().length > 1 ? ` · ${selection.size} selected` : "");
    if (selEntity.maxHp > 0) {
      hpwrap.classList.remove("hidden");
      hp.style.width = `${Math.max(0, Math.min(100, (selEntity.hp / selEntity.maxHp) * 100))}%`;
    } else {
      hpwrap.classList.add("hidden");
    }
    if (selEntity.queue && selEntity.queue.length > 0) {
      queue.classList.remove("hidden");
      queue.innerHTML =
        `Queue: ${selEntity.queue.join(" → ")}` +
        `<div class="queue-bar"><div style="width:${Math.round(
          (selEntity.buildTime ? selEntity.progress! / selEntity.buildTime : 0) * 100,
        )}%"></div></div>`;
    } else {
      queue.classList.add("hidden");
    }
  } else if (selection.size > 0) {
    const kinds = new Map<string, number>();
    for (const id of selection) {
      const e = world.entities.get(id);
      if (e) kinds.set(e.kind, (kinds.get(e.kind) ?? 0) + 1);
    }
    name.textContent = [...kinds.entries()].map(([k, n]) => `${n}× ${k}`).join(", ");
    detail.textContent = "Right-click to attack-move";
    hpwrap.classList.add("hidden");
    queue.classList.add("hidden");
  } else {
    name.textContent = "—";
    detail.textContent = "Select a unit or building.";
    hpwrap.classList.add("hidden");
    queue.classList.add("hidden");
  }

  // Command menu (build vs train).
  const grid = el("cmd-grid");
  const empty = el("cmd-empty");
  grid.innerHTML = "";
  if (selEntity && BUILDING_KINDS.has(selEntity.kind)) {
    empty.classList.add("hidden");
    for (const u of producibleUnits(selEntity.kind)) {
      grid.appendChild(cmdButton(u, UNIT_COSTS[u], () => sendCommands([trainUnit(single!, u)])));
    }
    if (selEntity.kind === "TechLab") {
      const dmg = cmdButton("Damage", 0, () => sendCommands([chooseUpgrade(single!, "Damage")]));
      (dmg.querySelector(".ic") as HTMLElement).textContent = "💥";
      grid.appendChild(dmg);
      const hpUp = cmdButton("Hp", 0, () => sendCommands([chooseUpgrade(single!, "Hp")]));
      (hpUp.querySelector(".ic") as HTMLElement).textContent = "❤";
      grid.appendChild(hpUp);
    }
    if (selEntity.kind !== "Hq") {
      const sellBtn = cmdButton("Sell", 0, () => sendCommands([sell(single!)]));
      (sellBtn.querySelector(".ic") as HTMLElement).textContent = "💲";
      grid.appendChild(sellBtn);
    }
  } else {
    empty.classList.remove("hidden");
    const buildings: BuildingType[] = ["Refinery", "Barracks", "Factory", "TechLab", "Turret"];
    for (const b of buildings) {
      grid.appendChild(
        cmdButton(b, BUILD_COSTS[b], () => {
          placementMode = placementMode === b ? null : b;
          placementCursor = null;
        }, { armed: placementMode === b }),
      );
    }
  }
}

function renderLog(): void {
  const log = el("log-body");
  log.innerHTML = "";
  for (const ev of world.events.slice(-6)) {
    const d = document.createElement("div");
    d.textContent = `${formatClock(ev.tick)} ${ev.kind}`;
    log.appendChild(d);
  }
}

function formatClock(tick: number): string {
  const s = Math.floor(tick / 10);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

// ---------------------------------------------------------------------------
// Loop
// ---------------------------------------------------------------------------

let radarSized = false;

function resize(): void {
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
  renderer.camera.setViewport(canvas.width, canvas.height);
  spectate.renderer.camera.setViewport(window.innerWidth, window.innerHeight);
  radarSized = false;
}
window.addEventListener("resize", resize);
resize();

let lastFrame = performance.now();

function frame(ts: number): void {
  const dt = Math.min(100, Math.max(1, ts - lastFrame));
  lastFrame = ts;

  ctx.fillStyle = "#04060a";
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  if (spectate.active) {
    spectate.draw(ctx, canvas.width, canvas.height);
    el("opponent").textContent = "spectating";
    el("ore").textContent = `${spectate.score0} · ${spectate.score1}`;
    el("clock").textContent = formatClock(spectate.currentTick);
    requestAnimationFrame(frame);
    return;
  }

  world.advance(dt);
  renderer.draw(ctx, world, selection, canvas.width, canvas.height);
  drawPlacementGhost();

  if (dragStart && dragCurrent) {
    ctx.strokeStyle = "#ffe27a";
    ctx.lineWidth = 1;
    const x = Math.min(dragStart[0], dragCurrent[0]);
    const y = Math.min(dragStart[1], dragCurrent[1]);
    ctx.strokeRect(x, y, Math.abs(dragCurrent[0] - dragStart[0]), Math.abs(dragCurrent[1] - dragStart[1]));
  }

  el("ore").textContent = String(world.ore);
  const refineries = world.ownBuildings.filter((b) => b.kind === "Refinery").length;
  el("income").textContent = refineries > 0 ? `+${refineries * 10}/s` : "";
  el("workers").textContent = String(world.ownUnits.filter((u) => u.kind === "Harvester").length);
  el("clock").textContent = formatClock(world.tick);
  renderCommandSidebar();
  renderLog();
  drawRadarFrame();
  requestAnimationFrame(frame);
}

// ---------------------------------------------------------------------------
// Radar (C&C-style sidebar minimap)
// ---------------------------------------------------------------------------

function sizeRadar(): void {
  const r = el<HTMLCanvasElement>("radar");
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(r.clientWidth * dpr));
  const h = Math.max(1, Math.round(r.clientHeight * dpr));
  if (r.width !== w || r.height !== h) {
    r.width = w;
    r.height = h;
    radarSized = true;
  }
}

function drawRadarFrame(): void {
  if (el("sidebar").classList.contains("hidden") || spectate.active) return;
  sizeRadar();
  if (!radarSized) return;
  const r = el<HTMLCanvasElement>("radar");
  const rctx = r.getContext("2d");
  if (!rctx) return;
  drawRadar(rctx, world, renderer.camera, selection, canvas.width, canvas.height);
}

// ---------------------------------------------------------------------------
// Placement preview
// ---------------------------------------------------------------------------

/// Mirror of the sim's `PLACE_RADIUS_TILES` (crates/crucible-sim/src/entity.rs):
/// a build site must be within 5 tiles of the nearest own building.
const PLACE_RADIUS = 5;

function canPlaceHere(tile: [number, number]): boolean {
  const [tx, ty] = tile;
  if (tx < 0 || ty < 0 || tx >= 64 || ty >= 64) return false;
  if (!world.passable[ty * 64 + tx]) return false;
  const ore = world.oreTiles.get(`${tx},${ty}`);
  if (ore && ore.amount > 0) return false;
  const blocked = [...world.entities.values()].some(
    (e) => e.owner === 0 && Math.floor(e.x) === tx && Math.floor(e.y) === ty,
  );
  if (blocked) return false;
  if (!placementMode) return false;
  const cost = BUILD_COSTS[placementMode] ?? 0;
  if (world.ore < cost) return false;
  return world.ownBuildings.some(
    (b) => (b.x - (tx + 0.5)) ** 2 + (b.y - (ty + 0.5)) ** 2 <= PLACE_RADIUS * PLACE_RADIUS,
  );
}

function drawPlacementGhost(): void {
  if (!placementMode || !placementCursor) return;
  const ok = canPlaceHere(placementCursor);
  const px = renderer.camera.screenX(placementCursor[0]);
  const py = renderer.camera.screenY(placementCursor[1]);
  const z = renderer.camera.zoom;
  if (px > canvas.width || py > canvas.height || px + z < 0 || py + z < 0) return;
  ctx.fillStyle = ok ? "rgba(74, 222, 128, 0.32)" : "rgba(248, 113, 113, 0.32)";
  ctx.fillRect(px, py, z, z);
  ctx.strokeStyle = ok ? "#4ade80" : "#f87171";
  ctx.lineWidth = 1.5;
  ctx.strokeRect(px + 0.75, py + 0.75, z - 1.5, z - 1.5);
  // A thin gold ring when the cursor is valid: signals "click to place".
  if (ok) {
    ctx.strokeStyle = "rgba(255, 215, 94, 0.9)";
    ctx.lineWidth = 1;
    ctx.strokeRect(px - 1.5, py - 1.5, z + 3, z + 3);
  }
}

// ---------------------------------------------------------------------------
// Hover tooltip (ore fields + buildings)
// ---------------------------------------------------------------------------

let tooltipVisible = false;

function updateTooltip(sx: number, sy: number): void {
  const tip = el("tooltip");
  const [tx, ty] = tileAt(sx, sy);
  const ore = world.oreTiles.get(`${tx},${ty}`);
  const ent = [...world.entities.values()].find(
    (e) => Math.floor(e.x) === tx && Math.floor(e.y) === ty,
  );
  let text = "";
  if (ent && ent.owner === 0) {
    text = `${ent.kind}` + (ent.maxHp > 0 ? ` · ${ent.hp}/${ent.maxHp} HP` : "");
  } else if (ore && ore.amount > 0) {
    text = `Ore field · ${ore.amount} remaining`;
  }
  if (text) {
    tip.textContent = text;
    tip.style.display = "block";
    tip.style.left = `${Math.min(sx + 14, window.innerWidth - 160)}px`;
    tip.style.top = `${sy + 14}px`;
    tooltipVisible = true;
  } else if (tooltipVisible) {
    tip.style.display = "none";
    tooltipVisible = false;
  }
}

canvas.addEventListener("mousemove", (ev) => {
  const [sx, sy] = canvasPos(ev);
  if (!spectate.active && !panning) updateTooltip(sx, sy);
});

// ---------------------------------------------------------------------------
// Opponent picker
// ---------------------------------------------------------------------------

interface ChampionInfo {
  genome_id: number;
  generation: number;
  reigning: boolean;
  elo: number | null;
}

async function initOpponentPicker(): Promise<void> {
  const championBtn = document.getElementById("champion-btn") as HTMLButtonElement | null;
  const museumRow = el("museum-opps");
  try {
    const [champRes, museumRes] = await Promise.all([
      fetch("/api/champion"),
      fetch("/api/museum"),
    ]);
    const champ = (await champRes.json()) as { champion: ChampionInfo | null };
    const museum = (await museumRes.json()) as { champions: ChampionInfo[] };

    if (championBtn) {
      if (champ.champion) {
        const c = champ.champion;
        const elo = c.elo == null ? "" : ` · Elo ${Math.round(c.elo)}`;
        championBtn.textContent = `Champion (gen ${c.generation}${elo})`;
        championBtn.disabled = false;
      } else {
        championBtn.textContent = "Champion (none crowned yet)";
        championBtn.disabled = true;
      }
    }

    museumRow.innerHTML = "";
    const bosses = museum.champions.filter((c) => !c.reigning).reverse().slice(0, 6);
    if (bosses.length > 0) {
      const label = document.createElement("span");
      label.className = "muted";
      label.textContent = "Museum bosses:";
      museumRow.appendChild(label);
      for (const c of bosses) {
        const b = document.createElement("button");
        b.className = "btn";
        b.textContent = `#${c.genome_id} (gen ${c.generation})`;
        b.addEventListener("click", () => startMatch(`museum:${c.genome_id}`, `champion #${c.genome_id}`));
        museumRow.appendChild(b);
      }
    }
  } catch {
    if (championBtn) {
      championBtn.textContent = "Champion (unavailable)";
      championBtn.disabled = true;
    }
  }
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

document.querySelectorAll<HTMLButtonElement>("[data-opp]").forEach((btn) => {
  btn.addEventListener("click", () => startMatch(btn.dataset.opp!, btn.dataset.label));
});
initDashboard();
spectate.init();
void initOpponentPicker();
el("again").addEventListener("click", () => {
  el("result").classList.add("hidden");
  el("lobby").classList.remove("hidden");
});

requestAnimationFrame(frame);
