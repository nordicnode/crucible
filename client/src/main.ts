// Client entry point: lobby, match loop, input, and HUD. All game logic is
// server-side; this only renders fogged state and forwards commands.

import { initDashboard } from "./dashboard";
import { Net } from "./net";
import { Renderer } from "./renderer";
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
let opponent = "hard";

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
      renderer.camera.cx = ownHq[0] + 0.5;
      renderer.camera.cy = ownHq[1] + 0.5;
      renderer.camera.zoom = 18;
      el("overlay").classList.add("hidden");
      el("build-panel").classList.remove("hidden");
      el("selection").classList.remove("hidden");
      el("log").classList.remove("hidden");
      el("opponent").textContent = `vs ${opponent}`;
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

function startMatch(which: string): void {
  opponent = which;
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
  el("build-panel").classList.add("hidden");
  el("selection").classList.add("hidden");
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
  if (ev.button === 0) {
    if (placementMode) {
      sendCommands([placeBuilding(placementMode, tileAt(sx, sy))]);
      placementMode = null;
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
    renderer.camera.pan(sx - lastPan[0], sy - lastPan[1]);
    lastPan = [sx, sy];
  } else if (dragStart) {
    dragCurrent = [sx, sy];
  }
});

canvas.addEventListener("mouseup", (ev) => {
  if (ev.button === 1) {
    panning = false;
    lastPan = null;
  }
  if (ev.button !== 0 || !dragStart) return;
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
  renderer.camera.zoomAt(sx, sy, ev.deltaY < 0 ? 1.15 : 1 / 1.15);
});

canvas.addEventListener("contextmenu", (ev) => ev.preventDefault());

window.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape") placementMode = null;
});

// ---------------------------------------------------------------------------
// HUD
// ---------------------------------------------------------------------------

let lastPanelSig = "";

function renderBuildPanelIfChanged(): void {
  const sig = [...selection].sort((a, b) => a - b).join(",") + "|" + placementMode;
  if (sig === lastPanelSig) return;
  lastPanelSig = sig;
  const panel = el("build-panel");
  panel.innerHTML = "";
  const single = selectedSingle();
  const selEntity = single != null ? world.entities.get(single) : null;

  if (selEntity && BUILDING_KINDS.has(selEntity.kind)) {
    for (const u of producibleUnits(selEntity.kind)) {
      panel.appendChild(costButton(u, UNIT_COSTS[u], () => sendCommands([trainUnit(single!, u)])));
    }
    if (selEntity.kind === "TechLab") {
      panel.appendChild(costButton("⚡ Damage", 0, () => sendCommands([chooseUpgrade(single!, "Damage")])));
      panel.appendChild(costButton("❤ HP", 0, () => sendCommands([chooseUpgrade(single!, "Hp")])));
    }
    if (selEntity.kind !== "Hq") {
      panel.appendChild(costButton("Sell", 0, () => sendCommands([sell(single!)])));
    }
  } else {
    const buildings: BuildingType[] = ["Refinery", "Barracks", "Factory", "TechLab", "Turret"];
    for (const b of buildings) {
      const btn = costButton(b, BUILD_COSTS[b], () => {
        placementMode = placementMode === b ? null : b;
      });
      if (placementMode === b) btn.classList.add("armed");
      panel.appendChild(btn);
    }
  }
}

function costButton(label: string, cost: number, onClick: () => void): HTMLButtonElement {
  const b = document.createElement("button");
  b.className = "btn";
  b.textContent = cost > 0 ? `${label} ${cost}` : label;
  b.addEventListener("click", onClick);
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

function renderSelectionPanel(): void {
  const panel = el("selection");
  if (selection.size === 0) {
    panel.classList.add("hidden");
    return;
  }
  panel.classList.remove("hidden");
  const units = selectedUnits().length;
  const kinds = new Map<string, number>();
  for (const id of selection) {
    const e = world.entities.get(id);
    if (e) kinds.set(e.kind, (kinds.get(e.kind) ?? 0) + 1);
  }
  panel.textContent =
    `${[...kinds.entries()].map(([k, n]) => `${n}× ${k}`).join(", ")}` +
    (units > 0 ? ` · right-click to attack-move` : "");
}

function renderLog(): void {
  const log = el("log");
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

function resize(): void {
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
}
window.addEventListener("resize", resize);
resize();

function frame(): void {
  ctx.fillStyle = "#04060a";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  renderer.draw(ctx, world, selection, canvas.width, canvas.height);

  if (dragStart && dragCurrent) {
    ctx.strokeStyle = "#ffe27a";
    ctx.lineWidth = 1;
    const x = Math.min(dragStart[0], dragCurrent[0]);
    const y = Math.min(dragStart[1], dragCurrent[1]);
    ctx.strokeRect(x, y, Math.abs(dragCurrent[0] - dragStart[0]), Math.abs(dragCurrent[1] - dragStart[1]));
  }

  el("ore").textContent = String(world.ore);
  el("clock").textContent = formatClock(world.tick);
  renderSelectionPanel();
  renderBuildPanelIfChanged();
  renderLog();
  requestAnimationFrame(frame);
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

document.querySelectorAll<HTMLButtonElement>("[data-opp]").forEach((btn) => {
  btn.addEventListener("click", () => startMatch(btn.dataset.opp!));
});
initDashboard();
el("again").addEventListener("click", () => {
  el("result").classList.add("hidden");
  el("lobby").classList.remove("hidden");
});

requestAnimationFrame(frame);
