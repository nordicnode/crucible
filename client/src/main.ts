// Client entry point: lobby, match loop, input, combat FX, and Command Sidebar.
// All simulation rules are server-side; this renders tactical state and forwards commands.

import { initDashboard } from "./dashboard";
import { fx } from "./fx";
import { IntelLogger } from "./intel";
import { Net } from "./net";
import { drawRadar, isBuildingPlacable, Renderer } from "./renderer";
import { spectate } from "./spectate";
import { getThumbnailDataUrl } from "./sprites";
import { World, type Entity } from "./world";
import {
  BUILDING_KINDS,
  BUILDING_POWER,
  BUILD_COSTS,
  UNIT_COSTS,
  UNIT_KINDS,
  chooseUpgrade,
  moveGroup,
  placeBuilding,
  repair,
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
const intel = new IntelLogger();

// Separate renderer, world, and state for background menu simulation
const menuRenderer = new Renderer();
const menuWorld = new World();
let menuInit = false;

interface DemoUnit {
  id: number;
  kind: string;
  owner: number;
  x: number;
  y: number;
  targetX: number;
  targetY: number;
  speed: number;
  fireCooldown: number;
  maxCooldown: number;
  hp: number;
  maxHp: number;
}

let demoUnits: DemoUnit[] = [];
let demoTime = 0;

let inGame = false;
let selection = new Set<number>();
let placementMode: BuildingType | null = null;
let placementCursor: [number, number] | null = null;
let opponentLabel = "hard";

// Rolling income tracker
const INCOME_WINDOW_TICKS = 100;
let incomeWindow: { tick: number; amount: number }[] = [];

// Waypoint destination tracking for tactical movement lines
const unitWaypoints = new Map<number, [number, number]>();

// Input drag and pan state
let dragStart: [number, number] | null = null;
let dragCurrent: [number, number] | null = null;
let panning = false;
let lastPan: [number, number] | null = null;
let minimapDragging = false;
const keysPressed = new Set<string>();

// Previous entity states for combat trigger detection
let prevEntityHp = new Map<number, number>();

function el<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

// ---------------------------------------------------------------------------
// Server messages
// ---------------------------------------------------------------------------

function onServerMsg(msg: ServerMsg): void {
  switch (msg.type) {
    case "matchStart": {
      inGame = true;
      world.setMap(msg.mapSeed, msg.passable, msg.hq);
      const ownHq = msg.hq[msg.player];
      renderer.camera.focusOn(ownHq[0] + 0.5, ownHq[1] + 0.5, 18, canvas.width, canvas.height);
      el("overlay").classList.add("hidden");
      el("lobby").classList.add("hidden");
      el("result").classList.add("hidden");
      el("dashboard").classList.add("hidden");
      el("spectate-list").classList.add("hidden");
      el("sidebar").classList.remove("hidden");
      el("topbar").classList.remove("hidden");
      el("log").classList.remove("hidden");
      el("opponent").textContent = opponentLabel.toUpperCase();
      unitWaypoints.clear();
      prevEntityHp.clear();
      intel.clear();
      intel.addEntry(0, "Tactical link active. Operation underway.", "info", "LINK");
      lastRenderedLogCount = -1;
      break;
    }
    case "stateDiff": {
      // Detect combat / attacks by observing entity damage & positions
      for (const e of msg.entities) {
        const prevHp = prevEntityHp.get(e.id);
        if (prevHp != null && e.hp < prevHp) {
          // Check for under-attack alert if friendly
          intel.processUnderAttack(msg.tick, e);

          // Entity took damage: find attacking enemies in combat range
          const attacker = [...world.entities.values()].find(
            (other) => other.owner !== e.owner && Math.hypot(other.x - e.x, other.y - e.y) <= 6.5,
          );
          if (attacker) {
            const kind = attacker.kind === "Artillery" ? "artillery" : attacker.kind === "Tank" ? "shell" : attacker.kind === "Turret" ? "laser" : "bullet";
            const color = attacker.owner === 0 ? "#7899a2" : "#b86d5d";
            fx.spawnAttack(attacker.x, attacker.y, e.x, e.y, kind, color);
            fx.recordUnitFiring(attacker.id, msg.tick);
          } else {
            fx.spawnImpactSparks(e.x, e.y, "#d6b44f");
          }
        }
        prevEntityHp.set(e.id, e.hp);

        // Detect Harvester harvesting and docking
        if (e.kind === "Harvester") {
          // Near an ore tile?
          const nearOre = [...world.oreTiles.values()].find((t) => t.amount > 0 && Math.hypot(t.x + 0.5 - e.x, t.y + 0.5 - e.y) < 1.2);
          if (nearOre) {
            fx.setMining(e.id, e.x, e.y, nearOre.x + 0.5, nearOre.y + 0.5);
          }
          // Near a friendly refinery?
          const nearRef = [...world.entities.values()].find((b) => b.kind === "Refinery" && b.owner === e.owner && Math.hypot(b.x - e.x, b.y - e.y) < 1.5);
          if (nearRef) {
            fx.setDocking(e.id, e.x, e.y, nearRef.x, nearRef.y);
          }
        }
      }

      // Check for destroyed entities
      for (const [id] of prevEntityHp) {
        if (!msg.entities.some((e) => e.id === id)) {
          const oldE = world.entities.get(id);
          if (oldE) {
            fx.spawnDeath(oldE.x, oldE.y, world.heading(id), oldE.kind, oldE.owner);
            intel.processEntityDestroyed(msg.tick, oldE);
          }
          prevEntityHp.delete(id);
        }
      }

      world.applyDiff(msg.tick, msg.ore, msg.entities, msg.oreTiles, msg.visible, msg.events);

      for (const ev of msg.events) {
        if (ev.kind === "ore_deposited" && (ev as unknown as { amount?: number }).amount != null) {
          incomeWindow.push({ tick: ev.tick, amount: (ev as unknown as { amount: number }).amount });
        }
        intel.processDiffEvent(ev);
      }
      incomeWindow = incomeWindow.filter((e) => e.tick >= msg.tick - INCOME_WINDOW_TICKS);
      break;
    }
    case "matchEnd": {
      inGame = false;
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
  inGame = false;
  opponentLabel = label ?? which;
  selection = new Set();
  placementMode = null;
  placementCursor = null;
  intel.clear();
  lastRenderedLogCount = -1;
  net.close();
  net.connect(onServerMsg, showLobby);
  net.send({ type: "joinMatch", opponent: which });
}

function showLobby(): void {
  inGame = false;
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
    for (const u of units) {
      unitWaypoints.set(u, tile);
    }
    return;
  }
  const single = selectedSingle();
  if (single != null) {
    const e = world.entities.get(single);
    if (e && BUILDING_KINDS.has(e.kind)) {
      sendCommands([setRally(single, tile)]);
      unitWaypoints.set(single, tile);
      e.rally = tile;
    }
  }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Radar Canvas Navigation (In Command Sidebar)
// ---------------------------------------------------------------------------

const radarCanvas = document.getElementById("radar") as HTMLCanvasElement | null;
const radarCtx = radarCanvas?.getContext("2d");

function radarTileAt(ev: MouseEvent): [number, number] | null {
  if (!radarCanvas) return null;
  const r = radarCanvas.getBoundingClientRect();
  const rx = ev.clientX - r.left;
  const ry = ev.clientY - r.top;
  const s = radarCanvas.width / 64;
  const tx = rx / s;
  const ty = ry / s;
  if (tx >= 0 && tx < 64 && ty >= 0 && ty < 64) {
    return [tx, ty];
  }
  return null;
}

if (radarCanvas) {
  radarCanvas.addEventListener("mousedown", (ev) => {
    if (ev.button === 0) {
      const tile = radarTileAt(ev);
      if (tile) {
        const cam = spectate.active ? spectate.renderer.camera : renderer.camera;
        cam.focusOn(tile[0], tile[1], cam.zoom, canvas.width, canvas.height);
      }
    }
  });

  radarCanvas.addEventListener("mousemove", (ev) => {
    if (ev.buttons === 1) {
      const tile = radarTileAt(ev);
      if (tile) {
        const cam = spectate.active ? spectate.renderer.camera : renderer.camera;
        cam.focusOn(tile[0], tile[1], cam.zoom, canvas.width, canvas.height);
      }
    }
  });
}

// ---------------------------------------------------------------------------
// Input Handling
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
    if (ev.button === 1 || ev.button === 0) {
      const mmPos = spectate.renderer.minimapToWorld(sx, sy, canvas.width, canvas.height);
      if (mmPos) {
        spectate.renderer.camera.centerOn(mmPos[0], mmPos[1], canvas.width, canvas.height);
        return;
      }
    }
    if (ev.button === 1) {
      panning = true;
      lastPan = [sx, sy];
    }
    return;
  }

  if (!inGame) return;

  // Main Game Canvas Input
  if (ev.button === 0) {
    const [tx, ty] = tileAt(sx, sy);
    if (toolMode === "sell") {
      const b = buildingAt(tx, ty);
      if (b && b.kind !== "Hq" && b.owner === 0) {
        sendCommands([sell(b.id)]);
      }
      return;
    }
    if (toolMode === "repair") {
      const b = buildingAt(tx, ty);
      if (b && b.hp < b.maxHp && b.owner === 0) {
        sendCommands([repair(b.id)]);
      }
      return;
    }
    if (placementMode) {
      if (isBuildingPlacable(placementMode, [tx, ty], world)) {
        sendCommands([placeBuilding(placementMode, [tx, ty])]);
        placementMode = null;
        placementCursor = null;
        lastPanelSig = "";
        renderCommandSidebar();
      }
      return;
    }
    dragStart = [sx, sy];
    dragCurrent = [sx, sy];
  } else if (ev.button === 1) {
    panning = true;
    lastPan = [sx, sy];
  } else if (ev.button === 2) {
    toolMode = null;
    placementMode = null;
    placementCursor = null;
    lastPanelSig = "";
    renderCommandSidebar();
    issueMove(tileAt(sx, sy));
  }
});

canvas.addEventListener("mousemove", (ev) => {
  const [sx, sy] = canvasPos(ev);

  if (spectate.active) {
    if (panning && lastPan) {
      spectate.renderer.camera.pan(sx - lastPan[0], sy - lastPan[1], canvas.width, canvas.height);
      lastPan = [sx, sy];
    }
    return;
  }

  if (!inGame) return;

  if (minimapDragging) {
    const mmPos = renderer.minimapToWorld(sx, sy, canvas.width, canvas.height);
    if (mmPos) {
      renderer.camera.centerOn(mmPos[0], mmPos[1], canvas.width, canvas.height);
    }
    return;
  }

  if (panning && lastPan) {
    renderer.camera.pan(sx - lastPan[0], sy - lastPan[1], canvas.width, canvas.height);
    lastPan = [sx, sy];
  } else if (dragStart) {
    dragCurrent = [sx, sy];
  }

  if (placementMode && !panning && !dragStart) {
    placementCursor = tileAt(sx, sy);
  }
});

canvas.addEventListener("mouseup", (ev) => {
  minimapDragging = false;
  if (ev.button === 1) {
    panning = false;
    lastPan = null;
  }
  if (!inGame || ev.button !== 0 || !dragStart) return;
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

  // First check if clicking directly ON an own building's tile
  const b = buildingAt(tx, ty);
  if (b && b.owner === 0) {
    bestId = b.id;
  } else {
    // Check if clicking near an own unit
    for (const e of world.ownUnits) {
      const dx = e.x - (tx + 0.5);
      const dy = e.y - (ty + 0.5);
      const d = dx * dx + dy * dy;
      if (d < 0.65 && d < bestDist) {
        bestDist = d;
        bestId = e.id;
      }
    }
  }

  if (bestId != null) {
    if (!additive) selection = new Set();
    selection.add(bestId);
  } else if (!additive) {
    selection = new Set();
  }
  lastPanelSig = "";
  renderCommandSidebar();
}

canvas.addEventListener("wheel", (ev) => {
  ev.preventDefault();
  const [sx, sy] = canvasPos(ev);
  if (spectate.active) {
    spectate.renderer.camera.zoomAt(sx, sy, ev.deltaY < 0 ? 1.18 : 1 / 1.18, canvas.width, canvas.height);
  } else if (inGame) {
    renderer.camera.zoomAt(sx, sy, ev.deltaY < 0 ? 1.18 : 1 / 1.18, canvas.width, canvas.height);
  }
});

canvas.addEventListener("contextmenu", (ev) => ev.preventDefault());

window.addEventListener("keydown", (ev) => {
  keysPressed.add(ev.code);
  if (ev.key === "Escape") {
    placementMode = null;
    placementCursor = null;
    toolMode = null;
    lastPanelSig = "";
    renderCommandSidebar();
  }
});

window.addEventListener("keyup", (ev) => {
  keysPressed.delete(ev.code);
});

// ---------------------------------------------------------------------------
// HUD & C&C Command Sidebar
// ---------------------------------------------------------------------------
// Command Matrix Tabs & Global Tool Modes
// ---------------------------------------------------------------------------

type CommandTab = "buildings" | "troops" | "vehicles" | "aircraft";
let activeTab: CommandTab = "buildings";
let toolMode: "sell" | "repair" | null = null;
let tabIconsInitialized = false;

function initToolAndTabIcons(): void {
  if (tabIconsInitialized) return;
  tabIconsInitialized = true;

  const repairImg = el("action-repair-img") as HTMLImageElement | null;
  if (repairImg) repairImg.src = getThumbnailDataUrl("repair", 0);
  const sellImg = el("action-sell-img") as HTMLImageElement | null;
  if (sellImg) sellImg.src = getThumbnailDataUrl("sell", 0);

  const bImg = el("tab-icon-buildings") as HTMLImageElement | null;
  if (bImg) bImg.src = getThumbnailDataUrl("tab_buildings", 0);
  const tImg = el("tab-icon-troops") as HTMLImageElement | null;
  if (tImg) tImg.src = getThumbnailDataUrl("tab_troops", 0);
  const vImg = el("tab-icon-vehicles") as HTMLImageElement | null;
  if (vImg) vImg.src = getThumbnailDataUrl("tab_vehicles", 0);
  const aImg = el("tab-icon-aircraft") as HTMLImageElement | null;
  if (aImg) aImg.src = getThumbnailDataUrl("tab_aircraft", 0);

  // Tab button click listeners
  for (const tabName of ["buildings", "troops", "vehicles", "aircraft"] as CommandTab[]) {
    const btn = el(`tab-btn-${tabName}`);
    if (btn) {
      btn.addEventListener("click", () => {
        activeTab = tabName;
        lastPanelSig = "";
        renderCommandSidebar();
      });
    }
  }

  // Tool actions click listeners (Repair / Sell)
  const repairBtn = el("action-repair");
  if (repairBtn) {
    repairBtn.addEventListener("click", () => {
      const single = selectedSingle();
      const selEntity = single != null ? world.entities.get(single) : null;
      if (
        selEntity &&
        BUILDING_KINDS.has(selEntity.kind) &&
        selEntity.owner === 0 &&
        selEntity.hp < selEntity.maxHp
      ) {
        sendCommands([repair(single!)]);
      } else {
        toolMode = toolMode === "repair" ? null : "repair";
        if (toolMode) placementMode = null;
        lastPanelSig = "";
        renderCommandSidebar();
      }
    });
  }

  const sellBtn = el("action-sell");
  if (sellBtn) {
    sellBtn.addEventListener("click", () => {
      const single = selectedSingle();
      const selEntity = single != null ? world.entities.get(single) : null;
      if (
        selEntity &&
        BUILDING_KINDS.has(selEntity.kind) &&
        selEntity.owner === 0 &&
        selEntity.kind !== "Hq"
      ) {
        sendCommands([sell(single!)]);
      } else {
        toolMode = toolMode === "sell" ? null : "sell";
        if (toolMode) placementMode = null;
        lastPanelSig = "";
        renderCommandSidebar();
      }
    });
  }
}

function buildingAt(tx: number, ty: number): Entity | null {
  for (const e of world.ownBuildings) {
    const bx = Math.floor(e.x);
    const by = Math.floor(e.y);
    if (tx === bx && ty === by) {
      return e;
    }
  }
  return null;
}

let lastPanelSig = "";

function cmdButton(
  key: string,
  cost: number,
  onClick: () => void,
  opts: {
    armed?: boolean;
    disabled?: boolean;
    disabledReason?: string;
    power?: { produces: number; consumes: number };
    label?: string;
  } = {},
): HTMLButtonElement {
  const thumbUrl = getThumbnailDataUrl(key, 0);
  const b = document.createElement("button");
  b.className = "cmd";
  const displayLabel = opts.label ?? key;
  b.innerHTML = `
    <img class="thumb" src="${thumbUrl}" alt="${key}" />
    <span class="label">${displayLabel}</span>
  `;
  if (cost > 0) {
    const c = document.createElement("span");
    c.className = "cost";
    c.textContent = String(cost);
    b.appendChild(c);
  }
  if (world.ore < cost || opts.disabled) {
    b.classList.add("disabled");
    if (opts.disabledReason) {
      b.title = opts.disabledReason;
    }
  }
  const pwr = opts.power ?? BUILDING_POWER[key];
  if (pwr) {
    const pTag = document.createElement("span");
    if (pwr.produces > 0) {
      pTag.className = "power-tag pos";
      pTag.textContent = `+${pwr.produces} PWR`;
      b.appendChild(pTag);
    } else if (pwr.consumes > 0) {
      pTag.className = "power-tag neg";
      pTag.textContent = `-${pwr.consumes} PWR`;
      b.appendChild(pTag);
    }
  }
  if (opts.armed) b.classList.add("armed");
  if (!b.classList.contains("disabled")) b.addEventListener("click", onClick);
  return b;
}

function renderCommandSidebar(): void {
  initToolAndTabIcons();

  const single = selectedSingle();
  const selEntity = single != null ? world.entities.get(single) : null;
  const qsig = selEntity && selEntity.queue ? `${selEntity.progress}/${selEntity.buildTime}` : "";
  const bCounts = `${world.ownBuildings.filter((b) => b.kind === "Barracks").length}-${
    world.ownBuildings.filter((b) => b.kind === "Factory").length
  }-${world.ownBuildings.filter((b) => b.kind === "TechLab").length}-${
    world.ownBuildings.filter((b) => b.kind === "Airfield").length
  }`;
  const sig =
    [...selection].sort((a, b) => a - b).join(",") +
    "|" +
    placementMode +
    "|" +
    toolMode +
    "|" +
    activeTab +
    "|" +
    qsig +
    "|" +
    world.ore +
    "|" +
    bCounts;
  if (sig === lastPanelSig) return;
  lastPanelSig = sig;

  // Update tab buttons active states
  for (const tab of ["buildings", "troops", "vehicles", "aircraft"] as CommandTab[]) {
    const btn = el(`tab-btn-${tab}`);
    if (btn) {
      if (activeTab === tab) btn.classList.add("active");
      else btn.classList.remove("active");
    }
  }

  // Update global tool mode buttons active highlights
  const repairBtn = el("action-repair");
  if (repairBtn) {
    if (toolMode === "repair") repairBtn.classList.add("armed");
    else repairBtn.classList.remove("armed");
  }
  const sellBtn = el("action-sell");
  if (sellBtn) {
    if (toolMode === "sell") sellBtn.classList.add("armed");
    else sellBtn.classList.remove("armed");
  }

  // Selection Card
  const name = el("sel-name");
  const detail = el("sel-detail");
  const hpwrap = el("sel-hpwrap");
  const hp = el("sel-hp");
  const queue = el("sel-queue");

  if (selEntity) {
    name.textContent = selEntity.kind.toUpperCase();
    const hpText = selEntity.maxHp > 0 ? `${selEntity.hp} / ${selEntity.maxHp} HP` : "";
    const pwr = BUILDING_POWER[selEntity.kind];
    const pwrText = pwr ? (pwr.produces > 0 ? ` · +${pwr.produces} PWR GEN` : ` · -${pwr.consumes} PWR DRAIN`) : "";
    detail.textContent = hpText + pwrText + (selectedUnits().length > 1 ? ` · ${selection.size} UNITS` : "");
    if (selEntity.maxHp > 0) {
      hpwrap.classList.remove("hidden");
      hp.style.width = `${Math.max(0, Math.min(100, (selEntity.hp / selEntity.maxHp) * 100))}%`;
    } else {
      hpwrap.classList.add("hidden");
    }
    if (selEntity.queue && selEntity.queue.length > 0) {
      queue.classList.remove("hidden");
      queue.innerHTML =
        `QUEUE: ${selEntity.queue.join(" → ").toUpperCase()}` +
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
    name.textContent = [...kinds.entries()].map(([k, n]) => `${n}× ${k.toUpperCase()}`).join(", ");
    detail.textContent = "RIGHT-CLICK TO ATTACK-MOVE";
    hpwrap.classList.add("hidden");
    queue.classList.add("hidden");
  } else {
    name.textContent = "NO SELECTION";
    detail.textContent = "SELECT BUILDING OR UNITS";
    hpwrap.classList.add("hidden");
    queue.classList.add("hidden");
  }

  // Command Menu Grid
  const grid = el("cmd-grid");
  const empty = el("cmd-empty");
  grid.innerHTML = "";
  empty.classList.add("hidden");

  // Contextual upgrades if TechLab is selected
  if (selEntity && selEntity.kind === "TechLab" && selEntity.owner === 0) {
    grid.appendChild(cmdButton("Damage", 0, () => sendCommands([chooseUpgrade(single!, "Damage")])));
    grid.appendChild(cmdButton("Hp", 0, () => sendCommands([chooseUpgrade(single!, "Hp")])));
  }

  if (activeTab === "buildings") {
    const hasFactory = world.ownBuildings.some((b) => b.kind === "Factory");
    const buildings: BuildingType[] = ["PowerPlant", "Refinery", "Barracks", "Factory", "TechLab", "Airfield", "Turret"];
    for (const b of buildings) {
      const isTech = b === "TechLab" || b === "Airfield";
      const isLocked = isTech && !hasFactory;
      grid.appendChild(
        cmdButton(
          b,
          BUILD_COSTS[b],
          () => {
            toolMode = null;
            placementMode = placementMode === b ? null : b;
            placementCursor = null;
            lastPanelSig = "";
            renderCommandSidebar();
          },
          {
            armed: placementMode === b,
            disabled: isLocked,
            disabledReason: isLocked ? "REQUIRES FACTORY" : undefined,
          },
        ),
      );
    }
  } else if (activeTab === "troops") {
    const barracks = world.ownBuildings.find((b) => b.kind === "Barracks");
    grid.appendChild(
      cmdButton(
        "Infantry",
        UNIT_COSTS["Infantry"],
        () => {
          if (barracks) {
            sendCommands([trainUnit(barracks.id, "Infantry")]);
          }
        },
        {
          disabled: !barracks,
          disabledReason: barracks ? undefined : "REQUIRES BARRACKS",
        },
      ),
    );
  } else if (activeTab === "vehicles") {
    const factory = world.ownBuildings.find((b) => b.kind === "Factory");
    const hasLab = world.ownBuildings.some((b) => b.kind === "TechLab");
    const vehicles: UnitType[] = ["Harvester", "Tank", "Artillery"];
    for (const v of vehicles) {
      const isArtillery = v === "Artillery";
      const isLocked = !factory || (isArtillery && !hasLab);
      const reason = !factory ? "REQUIRES FACTORY" : isArtillery && !hasLab ? "REQUIRES TECH LAB" : undefined;
      grid.appendChild(
        cmdButton(
          v,
          UNIT_COSTS[v],
          () => {
            if (factory && (!isArtillery || hasLab)) {
              sendCommands([trainUnit(factory.id, v)]);
            }
          },
          {
            disabled: isLocked,
            disabledReason: reason,
          },
        ),
      );
    }
  } else if (activeTab === "aircraft") {
    const airfield = world.ownBuildings.find((b) => b.kind === "Airfield");
    grid.appendChild(
      cmdButton(
        "Gunship",
        120,
        () => {},
        {
          disabled: !airfield,
          disabledReason: airfield ? undefined : "REQUIRES AIRFIELD",
        },
      ),
    );
    grid.appendChild(
      cmdButton(
        "Interceptor",
        150,
        () => {},
        {
          disabled: !airfield,
          disabledReason: airfield ? undefined : "REQUIRES AIRFIELD",
        },
      ),
    );
  }
}

let lastRenderedLogCount = -1;

function renderLog(): void {
  const log = el("log-body");
  if (intel.entries.length === lastRenderedLogCount) {
    return;
  }
  lastRenderedLogCount = intel.entries.length;
  log.innerHTML = "";
  for (const entry of intel.entries) {
    const row = document.createElement("div");
    row.className = "log-entry";

    const timeSpan = document.createElement("span");
    timeSpan.className = "log-time";
    timeSpan.textContent = `[${formatClock(entry.tick)}]`;

    const tagSpan = document.createElement("span");
    tagSpan.className = `log-tag log-tag-${entry.level}`;
    tagSpan.textContent = entry.tag;

    const msgSpan = document.createElement("span");
    msgSpan.className = `log-msg log-msg-${entry.level}`;
    msgSpan.textContent = entry.text;

    row.appendChild(timeSpan);
    row.appendChild(tagSpan);
    row.appendChild(msgSpan);
    log.appendChild(row);
  }
  log.scrollTop = log.scrollHeight;
}

function formatClock(tick: number): string {
  const s = Math.floor(tick / 10);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

// ---------------------------------------------------------------------------
// Main Render Loop & Menu Battle Theater
// ---------------------------------------------------------------------------

function resize(): void {
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
  renderer.camera.setViewport(canvas.width, canvas.height);
  menuRenderer.camera.setViewport(canvas.width, canvas.height);
  spectate.renderer.camera.setViewport(window.innerWidth, window.innerHeight);
}
window.addEventListener("resize", resize);
resize();

function initMenuBattle(): void {
  menuWorld.setMap(42, new Array(64 * 64).fill(true), [[10, 10], [54, 54]]);
  menuWorld.oreTiles.set("18,18", { x: 18, y: 18, amount: 600 });
  menuWorld.oreTiles.set("46,46", { x: 46, y: 46, amount: 600 });
  menuWorld.oreTiles.set("32,32", { x: 32, y: 32, amount: 800 });
  menuWorld.visible = new Set(Array.from({ length: 64 * 64 }, (_, i) => i));

  demoUnits = [
    // P0 Blue base & army
    { id: 1, kind: "Hq", owner: 0, x: 10.5, y: 10.5, targetX: 10.5, targetY: 10.5, speed: 0, fireCooldown: 999, maxCooldown: 999, hp: 1500, maxHp: 1500 },
    { id: 2, kind: "Refinery", owner: 0, x: 14.5, y: 10.5, targetX: 14.5, targetY: 10.5, speed: 0, fireCooldown: 999, maxCooldown: 999, hp: 400, maxHp: 400 },
    { id: 3, kind: "Factory", owner: 0, x: 10.5, y: 14.5, targetX: 10.5, targetY: 14.5, speed: 0, fireCooldown: 999, maxCooldown: 999, hp: 350, maxHp: 350 },
    { id: 4, kind: "Turret", owner: 0, x: 22.5, y: 18.5, targetX: 22.5, targetY: 18.5, speed: 0, fireCooldown: 30, maxCooldown: 40, hp: 200, maxHp: 200 },
    { id: 5, kind: "Harvester", owner: 0, x: 18.5, y: 18.5, targetX: 18.5, targetY: 18.5, speed: 1.2, fireCooldown: 999, maxCooldown: 999, hp: 80, maxHp: 80 },
    { id: 6, kind: "Tank", owner: 0, x: 26.5, y: 26.5, targetX: 34.5, targetY: 30.5, speed: 1.8, fireCooldown: 15, maxCooldown: 35, hp: 120, maxHp: 120 },
    { id: 7, kind: "Tank", owner: 0, x: 25.5, y: 29.5, targetX: 33.5, targetY: 33.5, speed: 1.8, fireCooldown: 25, maxCooldown: 35, hp: 120, maxHp: 120 },
    { id: 8, kind: "Artillery", owner: 0, x: 20.5, y: 27.5, targetX: 24.5, targetY: 27.5, speed: 1.0, fireCooldown: 40, maxCooldown: 60, hp: 60, maxHp: 60 },
    { id: 9, kind: "Infantry", owner: 0, x: 27.5, y: 24.5, targetX: 32.5, targetY: 28.5, speed: 2.2, fireCooldown: 10, maxCooldown: 20, hp: 44, maxHp: 44 },
    { id: 10, kind: "Infantry", owner: 0, x: 28.5, y: 25.5, targetX: 33.5, targetY: 29.5, speed: 2.2, fireCooldown: 15, maxCooldown: 20, hp: 44, maxHp: 44 },

    // P1 Red base & army
    { id: 11, kind: "Hq", owner: 1, x: 53.5, y: 53.5, targetX: 53.5, targetY: 53.5, speed: 0, fireCooldown: 999, maxCooldown: 999, hp: 1500, maxHp: 1500 },
    { id: 12, kind: "Refinery", owner: 1, x: 49.5, y: 53.5, targetX: 49.5, targetY: 53.5, speed: 0, fireCooldown: 999, maxCooldown: 999, hp: 400, maxHp: 400 },
    { id: 13, kind: "Turret", owner: 1, x: 41.5, y: 45.5, targetX: 41.5, targetY: 45.5, speed: 0, fireCooldown: 20, maxCooldown: 40, hp: 200, maxHp: 200 },
    { id: 14, kind: "Harvester", owner: 1, x: 46.5, y: 46.5, targetX: 46.5, targetY: 46.5, speed: 1.2, fireCooldown: 999, maxCooldown: 999, hp: 80, maxHp: 80 },
    { id: 15, kind: "Tank", owner: 1, x: 37.5, y: 33.5, targetX: 29.5, targetY: 28.5, speed: 1.8, fireCooldown: 10, maxCooldown: 35, hp: 120, maxHp: 120 },
    { id: 16, kind: "Tank", owner: 1, x: 38.5, y: 30.5, targetX: 30.5, targetY: 26.5, speed: 1.8, fireCooldown: 20, maxCooldown: 35, hp: 120, maxHp: 120 },
    { id: 17, kind: "Artillery", owner: 1, x: 43.5, y: 35.5, targetX: 40.5, targetY: 35.5, speed: 1.0, fireCooldown: 30, maxCooldown: 60, hp: 60, maxHp: 60 },
    { id: 18, kind: "Infantry", owner: 1, x: 36.5, y: 35.5, targetX: 31.5, targetY: 31.5, speed: 2.2, fireCooldown: 5, maxCooldown: 20, hp: 44, maxHp: 44 },
    { id: 19, kind: "Infantry", owner: 1, x: 35.5, y: 36.5, targetX: 30.5, targetY: 32.5, speed: 2.2, fireCooldown: 12, maxCooldown: 20, hp: 44, maxHp: 44 },
  ];

  menuInit = true;
}

function updateMenuBattle(dtSec: number): void {
  demoTime += dtSec;
  menuWorld.tick++;

  // Advance units on patrol/skirmish paths
  for (const u of demoUnits) {
    if (u.speed > 0) {
      const dx = u.targetX - u.x;
      const dy = u.targetY - u.y;
      const dist = Math.hypot(dx, dy);
      if (dist > 0.15) {
        const moveDist = Math.min(dist, u.speed * dtSec);
        u.x += (dx / dist) * moveDist;
        u.y += (dy / dist) * moveDist;
        const heading = Math.atan2(dy, dx);
        menuWorld.headings.set(u.id, heading);
      } else {
        // Pick next patrol waypoint in battlefield center
        if (u.kind === "Harvester") {
          const oreX = u.owner === 0 ? 18.5 : 46.5;
          const oreY = u.owner === 0 ? 18.5 : 46.5;
          const refX = u.owner === 0 ? 14.5 : 49.5;
          const refY = u.owner === 0 ? 10.5 : 53.5;
          if (Math.hypot(u.x - oreX, u.y - oreY) < 1.5) {
            u.targetX = refX;
            u.targetY = refY;
            fx.setMining(u.id, u.x, u.y, oreX, oreY);
          } else {
            u.targetX = oreX;
            u.targetY = oreY;
            fx.setDocking(u.id, u.x, u.y, refX, refY);
          }
        } else {
          // Combat units skirmish back and forth in center
          const minX = u.owner === 0 ? 25 : 34;
          const maxX = u.owner === 0 ? 32 : 42;
          const minY = 26;
          const maxY = 36;
          u.targetX = minX + Math.random() * (maxX - minX);
          u.targetY = minY + Math.random() * (maxY - minY);
        }
      }
    }

    // Combat shooting
    u.fireCooldown -= dtSec * 10;
    if (u.fireCooldown <= 0) {
      u.fireCooldown = u.maxCooldown * (0.8 + Math.random() * 0.4);
      const enemy = demoUnits.find((other) => other.owner !== u.owner && Math.hypot(other.x - u.x, other.y - u.y) < 10);
      if (enemy) {
        const projKind = u.kind === "Artillery" ? "artillery" : u.kind === "Tank" ? "shell" : u.kind === "Turret" ? "laser" : "bullet";
        const projColor = u.owner === 0 ? "#7899a2" : "#b86d5d";
        fx.spawnAttack(u.x, u.y, enemy.x, enemy.y, projKind, projColor);
        fx.recordUnitFiring(u.id, menuWorld.tick);
      }
    }
  }

  // Sync with menuWorld entities
  const entities = demoUnits.map((u) => ({
    id: u.id,
    kind: u.kind,
    owner: u.owner,
    x: u.x,
    y: u.y,
    hp: u.hp,
    maxHp: u.maxHp,
  }));

  menuWorld.applyDiff(
    menuWorld.tick,
    500,
    entities,
    [{ x: 18, y: 18, amount: 600 }, { x: 46, y: 46, amount: 600 }, { x: 32, y: 32, amount: 800 }],
    Array.from({ length: 64 * 64 }, (_, i) => i),
    [],
  );
}

let lastFrame = performance.now();

function frame(ts: number): void {
  const dt = Math.min(100, Math.max(1, ts - lastFrame));
  const dtSec = dt / 1000;
  lastFrame = ts;

  // Step FX system
  fx.update(dtSec);

  // Keyboard camera panning in-game or in-spectate
  const panSpeed = (400 * dt) / 1000;
  if (spectate.active) {
    if (keysPressed.has("KeyW") || keysPressed.has("ArrowUp")) spectate.renderer.camera.pan(0, panSpeed, canvas.width, canvas.height);
    if (keysPressed.has("KeyS") || keysPressed.has("ArrowDown")) spectate.renderer.camera.pan(0, -panSpeed, canvas.width, canvas.height);
    if (keysPressed.has("KeyA") || keysPressed.has("ArrowLeft")) spectate.renderer.camera.pan(panSpeed, 0, canvas.width, canvas.height);
    if (keysPressed.has("KeyD") || keysPressed.has("ArrowRight")) spectate.renderer.camera.pan(-panSpeed, 0, canvas.width, canvas.height);
  } else if (inGame) {
    if (keysPressed.has("KeyW") || keysPressed.has("ArrowUp")) renderer.camera.pan(0, panSpeed, canvas.width, canvas.height);
    if (keysPressed.has("KeyS") || keysPressed.has("ArrowDown")) renderer.camera.pan(0, -panSpeed, canvas.width, canvas.height);
    if (keysPressed.has("KeyA") || keysPressed.has("ArrowLeft")) renderer.camera.pan(panSpeed, 0, canvas.width, canvas.height);
    if (keysPressed.has("KeyD") || keysPressed.has("ArrowRight")) renderer.camera.pan(-panSpeed, 0, canvas.width, canvas.height);
  }

  ctx.fillStyle = "#04060a";
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  if (spectate.active) {
    spectate.draw(ctx, canvas.width, canvas.height);
    if (radarCtx && radarCanvas) {
      drawRadar(radarCtx, spectate.world, new Set(), spectate.renderer.camera, radarCanvas.width, radarCanvas.height);
    }
    el("opponent").textContent = "SPECTATING REPLAY";
    el("ore").textContent = `${spectate.score0} · ${spectate.score1}`;
    el("clock").textContent = formatClock(spectate.currentTick);
    requestAnimationFrame(frame);
    return;
  }

  if (!inGame) {
    // Menu background simulation only (no radar on menu movie)
    if (!menuInit) initMenuBattle();
    updateMenuBattle(dtSec);

    // Zoom in on the central battlefield action with smooth cinematic camera orbit
    const sweepAngle = demoTime * 0.12;
    const sweepX = 31 + Math.cos(sweepAngle) * 5;
    const sweepY = 30 + Math.sin(sweepAngle) * 4;
    menuRenderer.camera.focusOn(sweepX, sweepY, 30, canvas.width, canvas.height);

    menuRenderer.draw(ctx, menuWorld, new Set(), canvas.width, canvas.height);
  } else {
    // Clean up completed unit waypoints
    for (const [uid, wp] of unitWaypoints) {
      const e = world.entities.get(uid);
      if (!e || Math.hypot(e.x - (wp[0] + 0.5), e.y - (wp[1] + 0.5)) < 0.6) {
        unitWaypoints.delete(uid);
      }
    }

    world.advance(dt);
    renderer.draw(ctx, world, selection, canvas.width, canvas.height, {
      waypoints: unitWaypoints,
      placementMode,
      placementCursor,
    });

    // Draw live Radar Surveillance in Command Sidebar
    if (radarCtx && radarCanvas) {
      drawRadar(radarCtx, world, selection, renderer.camera, radarCanvas.width, radarCanvas.height);
    }

    // Box selection drag rectangle
    if (dragStart && dragCurrent) {
      ctx.strokeStyle = "rgba(255, 226, 122, 0.85)";
      ctx.lineWidth = 1.5;
      ctx.setLineDash([4, 4]);
      const x = Math.min(dragStart[0], dragCurrent[0]);
      const y = Math.min(dragStart[1], dragCurrent[1]);
      ctx.strokeRect(x, y, Math.abs(dragCurrent[0] - dragStart[0]), Math.abs(dragCurrent[1] - dragStart[1]));
      ctx.setLineDash([]);
    }

    el("ore").textContent = String(world.ore);
    const sumIncome = incomeWindow.reduce((acc, c) => acc + c.amount, 0);
    const ratePerSec = Math.round((sumIncome / (INCOME_WINDOW_TICKS / 10)) * 10) / 10;
    el("income").textContent = ratePerSec > 0 ? `+${ratePerSec}/S` : "";

    const pwr = world.ownPower;
    el("power-used").textContent = String(pwr.consumed);
    el("power-total").textContent = String(pwr.produced);
    const pwrRes = document.getElementById("power-res");
    if (pwrRes) {
      if (pwr.consumed > pwr.produced) {
        pwrRes.classList.add("low-power");
      } else {
        pwrRes.classList.remove("low-power");
      }
    }
    intel.processPowerStatus(world.tick, pwr.produced, pwr.consumed);

    el("workers").textContent = String(world.ownUnits.filter((u) => u.kind === "Harvester").length);
    el("clock").textContent = formatClock(world.tick);
    renderCommandSidebar();
    renderLog();
  }

  requestAnimationFrame(frame);
}

// ---------------------------------------------------------------------------
// Opponent Picker
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
        const elo = c.elo == null ? "" : ` · ELO ${Math.round(c.elo)}`;
        championBtn.textContent = `CHAMPION (GEN ${c.generation}${elo})`;
        championBtn.disabled = false;
      } else {
        championBtn.textContent = "CHAMPION (NONE CROWNED)";
        championBtn.disabled = true;
      }
    }

    museumRow.innerHTML = "";
    const bosses = museum.champions.filter((c) => !c.reigning).reverse().slice(0, 6);
    if (bosses.length > 0) {
      const label = document.createElement("span");
      label.className = "muted";
      label.textContent = "MUSEUM ARCHIVES:";
      museumRow.appendChild(label);
      for (const c of bosses) {
        const b = document.createElement("button");
        b.className = "btn";
        b.textContent = `#${c.genome_id} (GEN ${c.generation})`;
        b.addEventListener("click", () => startMatch(`museum:${c.genome_id}`, `CHAMPION #${c.genome_id}`));
        museumRow.appendChild(b);
      }
    }
  } catch {
    if (championBtn) {
      championBtn.textContent = "CHAMPION (OFFLINE)";
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
initToolAndTabIcons();
spectate.init();
void initOpponentPicker();

el("again").addEventListener("click", () => {
  showLobby();
});

requestAnimationFrame(frame);
