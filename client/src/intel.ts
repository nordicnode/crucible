// Combat Intelligence tactical logging system.
// Filters out noise (e.g. ore deposits) and provides clear, prioritized RTS alerts.

import type { DiffEntity, DiffEvent } from "./types";

export type LogLevel = "info" | "prod" | "warn" | "danger" | "kill";

export interface IntelLogEntry {
  id: number;
  tick: number;
  text: string;
  level: LogLevel;
  tag: string;
}

export function formatClock(tick: number): string {
  const s = Math.floor(Math.max(0, tick) / 10);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function friendlyBuildingCompleteMsg(btype: string): string {
  const norm = btype.toLowerCase();
  switch (norm) {
    case "powerplant":
      return "Power Generator online";
    case "refinery":
      return "Refinery constructed";
    case "barracks":
      return "Barracks online";
    case "factory":
      return "Factory operational";
    case "turret":
      return "Defense Turret online";
    case "techlab":
      return "TechLab active";
    case "airfield":
      return "Airfield online";
    case "radar":
      return "Radar array online";
    case "teslacoil":
      return "Tesla Coil charged";
    default:
      return `${btype} complete`;
  }
}

export function friendlyUnitReadyMsg(utype: string): string {
  const norm = utype.toLowerCase();
  switch (norm) {
    case "harvester":
      return "Harvester deployed";
    case "infantry":
      return "Infantry squad ready";
    case "tank":
      return "Tank roll out";
    case "artillery":
      return "Artillery operational";
    case "gunship":
      return "Gunship airborne";
    case "interceptor":
      return "Interceptor scrambled";
    case "mammothtank":
      return "Mammoth Tank deployed";
    default:
      return `${utype} ready for orders`;
  }
}

export class IntelLogger {
  private nextId = 1;
  entries: IntelLogEntry[] = [];
  readonly maxEntries: number;

  // Throttling for attack warnings (in ticks, 10 ticks = 1 second)
  private lastAlertPerEntity = new Map<number, number>();
  private lastCategoryAlertTick = new Map<string, number>();

  constructor(maxEntries = 40) {
    this.maxEntries = maxEntries;
  }

  clear(): void {
    this.entries = [];
    this.lastAlertPerEntity.clear();
    this.lastCategoryAlertTick.clear();
    this.nextId = 1;
  }

  addEntry(tick: number, text: string, level: LogLevel, tag: string): IntelLogEntry {
    const entry: IntelLogEntry = {
      id: this.nextId++,
      tick,
      text,
      level,
      tag,
    };
    this.entries.push(entry);
    if (this.entries.length > this.maxEntries) {
      this.entries = this.entries.slice(-this.maxEntries);
    }
    return entry;
  }

  /**
   * Process incoming Server DiffEvents. Returns added entry if any.
   *
   * The server only sends events belonging to the human player (P0), so the
   * friendly branches below are the only ones that can ever fire; enemy
   * activity arrives via fog observations and entity-destruction detection
   * instead.
   */
  processDiffEvent(ev: DiffEvent): IntelLogEntry | null {
    // Explicitly ignore spammy economy events (ore deposits)
    if (ev.kind === "ore_deposited") {
      return null;
    }

    // Building placed / constructed
    if (ev.kind.startsWith("built:")) {
      const btype = ev.kind.slice(6);
      return this.addEntry(
        ev.tick,
        friendlyBuildingCompleteMsg(btype),
        "prod",
        "BASE",
      );
    }

    // Unit trained / complete
    if (ev.kind.startsWith("trained:")) {
      const utype = ev.kind.slice(8);
      return this.addEntry(ev.tick, friendlyUnitReadyMsg(utype), "prod", "UNIT");
    }

    // Upgrade chosen / researched
    if (ev.kind.startsWith("upgrade")) {
      let msg = "Upgrade research complete";
      if (ev.kind === "upgrade:damage") {
        msg = "Upgrade complete: High-Explosive (+15% Dmg)";
      } else if (ev.kind === "upgrade:hp") {
        msg = "Upgrade complete: Reinforced Armor (+15% HP)";
      } else if (ev.kind === "upgrade:range") {
        msg = "Upgrade complete: Extended Range (+20% Range)";
      }
      return this.addEntry(ev.tick, msg, "prod", "TECH");
    }

    // Structure sold / decommissioned
    if (ev.kind === "sold") {
      const refund = ev.amount != null ? ` (+${ev.amount} ore)` : "";
      return this.addEntry(ev.tick, `Structure sold${refund}`, "info", "SOLD");
    }

    return null;
  }

  /**
   * Check if a friendly entity taking damage warrants an "Under Attack" alert.
   * Debounces repeated hits on the same unit/structure or rapid spam.
   */
  processUnderAttack(tick: number, entity: DiffEntity): IntelLogEntry | null {
    if (entity.owner !== 0) return null;

    const lastAlert = this.lastAlertPerEntity.get(entity.id) ?? -9999;
    const ENTITY_COOLDOWN = 30; // 3 seconds per specific entity

    if (tick - lastAlert < ENTITY_COOLDOWN) {
      return null;
    }

    let category = "unit";
    let text = "";
    let level: LogLevel = "warn";
    let tag = "ATTACK";

    if (entity.kind === "Harvester") {
      category = "harvester";
      text = "WARNING: Harvester under attack!";
      level = "warn";
      tag = "WARN";
    } else if (entity.kind === "Hq") {
      category = "hq";
      text = "ALERT: Base HQ under attack!";
      level = "danger";
      tag = "ALERT";
    } else if (["Refinery", "Barracks", "Factory", "TechLab", "Airfield", "Radar", "TeslaCoil", "Turret"].includes(entity.kind)) {
      category = `building_${entity.kind}`;
      text = `ALERT: ${entity.kind} under fire!`;
      level = "danger";
      tag = "ALERT";
    } else {
      category = `combat_${entity.kind}`;
      text = `Forces under attack (${entity.kind})`;
      level = "warn";
      tag = "ATTACK";
    }

    const lastCatAlert = this.lastCategoryAlertTick.get(category) ?? -9999;
    const CATEGORY_COOLDOWN = 20; // 2 seconds per alert category
    if (tick - lastCatAlert < CATEGORY_COOLDOWN) {
      return null;
    }

    this.lastAlertPerEntity.set(entity.id, tick);
    this.lastCategoryAlertTick.set(category, tick);

    return this.addEntry(tick, text, level, tag);
  }

  /**
   * Process entity destruction / loss.
   */
  processEntityDestroyed(tick: number, entity: { id: number; kind: string; owner: number }): IntelLogEntry {
    const isFriendly = entity.owner === 0;

    if (isFriendly) {
      if (entity.kind === "Hq") {
        return this.addEntry(tick, "CRITICAL: Base HQ destroyed!", "danger", "LOST");
      }
      if (["PowerPlant", "Refinery", "Barracks", "Factory", "TechLab", "Airfield", "Radar", "TeslaCoil", "Turret"].includes(entity.kind)) {
        return this.addEntry(tick, `CRITICAL: ${entity.kind} destroyed!`, "danger", "LOST");
      }
      if (entity.kind === "Harvester") {
        return this.addEntry(tick, "CRITICAL: Harvester lost!", "danger", "LOST");
      }
      return this.addEntry(tick, `Unit lost: ${entity.kind}`, "danger", "LOST");
    } else {
      if (["Hq", "PowerPlant", "Refinery", "Barracks", "Factory", "TechLab", "Airfield", "Radar", "TeslaCoil", "Turret"].includes(entity.kind)) {
        return this.addEntry(tick, `Enemy ${entity.kind} destroyed!`, "kill", "KILL");
      }
      return this.addEntry(tick, `Hostile neutralized: ${entity.kind}`, "kill", "KILL");
    }
  }

  /**
   * Alert commander if power consumption exceeds production.
   */
  processPowerStatus(tick: number, powerProduced: number, powerConsumed: number): IntelLogEntry | null {
    if (powerConsumed > powerProduced) {
      const last = this.lastCategoryAlertTick.get("low_power") ?? -9999;
      if (tick - last >= 100) {
        // At most once every 10 seconds (100 ticks)
        this.lastCategoryAlertTick.set("low_power", tick);
        return this.addEntry(tick, `LOW POWER WARNING: ${powerConsumed}/${powerProduced} PWR (50% Production Speed)`, "warn", "POWER");
      }
    }
    return null;
  }
}
