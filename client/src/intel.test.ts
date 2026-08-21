import { describe, expect, it } from "vitest";
import {
  IntelLogger,
  formatClock,
  friendlyBuildingCompleteMsg,
  friendlyUnitReadyMsg,
} from "./intel";
import type { DiffEntity, DiffEvent } from "./types";

describe("Intel formatters", () => {
  it("formats clock ticks into mm:ss", () => {
    expect(formatClock(0)).toBe("0:00");
    expect(formatClock(95)).toBe("0:09");
    expect(formatClock(600)).toBe("1:00");
    expect(formatClock(1250)).toBe("2:05");
  });

  it("formats friendly building completion messages", () => {
    expect(friendlyBuildingCompleteMsg("PowerPlant")).toBe("Power Generator online");
    expect(friendlyBuildingCompleteMsg("Refinery")).toBe("Refinery constructed");
    expect(friendlyBuildingCompleteMsg("Barracks")).toBe("Barracks online");
    expect(friendlyBuildingCompleteMsg("Factory")).toBe("Factory operational");
    expect(friendlyBuildingCompleteMsg("Turret")).toBe("Defense Turret online");
    expect(friendlyBuildingCompleteMsg("TechLab")).toBe("TechLab active");
    expect(friendlyBuildingCompleteMsg("Custom")).toBe("Custom complete");
  });

  it("formats friendly unit ready messages", () => {
    expect(friendlyUnitReadyMsg("Harvester")).toBe("Harvester deployed");
    expect(friendlyUnitReadyMsg("Infantry")).toBe("Infantry squad ready");
    expect(friendlyUnitReadyMsg("Tank")).toBe("Tank roll out");
    expect(friendlyUnitReadyMsg("Artillery")).toBe("Artillery operational");
    expect(friendlyUnitReadyMsg("Scout")).toBe("Scout ready for orders");
  });
});

describe("IntelLogger", () => {
  it("ignores ore_deposited events to prevent spam", () => {
    const logger = new IntelLogger();
    const ev: DiffEvent = { tick: 100, kind: "ore_deposited", amount: 25 };
    const res = logger.processDiffEvent(ev);
    expect(res).toBeNull();
    expect(logger.entries.length).toBe(0);
  });

  it("logs friendly building completion", () => {
    const logger = new IntelLogger();
    const ev: DiffEvent = { tick: 150, kind: "built:refinery", player: 0 };
    const entry = logger.processDiffEvent(ev);
    expect(entry).not.toBeNull();
    expect(entry?.text).toBe("Refinery constructed");
    expect(entry?.level).toBe("prod");
    expect(entry?.tag).toBe("BASE");
  });

  it("logs friendly unit completion", () => {
    const logger = new IntelLogger();
    const ev: DiffEvent = { tick: 200, kind: "trained:tank", player: 0 };
    const entry = logger.processDiffEvent(ev);
    expect(entry).not.toBeNull();
    expect(entry?.text).toBe("Tank roll out");
    expect(entry?.level).toBe("prod");
    expect(entry?.tag).toBe("UNIT");
  });

  it("logs training completions for any event the server sends", () => {
    // The server only ever sends P0 events, so processDiffEvent treats every
    // incoming event as friendly; enemy activity arrives via fog instead.
    const logger = new IntelLogger();
    const ev: DiffEvent = { tick: 200, kind: "trained:tank" };
    const entry = logger.processDiffEvent(ev);
    expect(entry).not.toBeNull();
    expect(entry?.text).toBe("Tank roll out");
    expect(entry?.tag).toBe("UNIT");
  });

  it("logs upgrade completions", () => {
    const logger = new IntelLogger();
    const evDmg: DiffEvent = { tick: 300, kind: "upgrade:damage", player: 0 };
    const resDmg = logger.processDiffEvent(evDmg);
    expect(resDmg?.text).toContain("High-Explosive");

    const evHp: DiffEvent = { tick: 400, kind: "upgrade:hp", player: 0 };
    const resHp = logger.processDiffEvent(evHp);
    expect(resHp?.text).toContain("Reinforced Armor");
  });

  it("logs structure sales", () => {
    const logger = new IntelLogger();
    const ev: DiffEvent = { tick: 250, kind: "sold", amount: 150, player: 0 };
    const entry = logger.processDiffEvent(ev);
    expect(entry?.text).toBe("Structure sold (+150 ore)");
    expect(entry?.tag).toBe("SOLD");
  });

  it("triggers under-attack alerts with intelligent throttling", () => {
    const logger = new IntelLogger();
    const harvester: DiffEntity = { id: 5, kind: "Harvester", owner: 0, x: 10, y: 10, hp: 60, maxHp: 80 };

    // Initial attack at tick 100
    const alert1 = logger.processUnderAttack(100, harvester);
    expect(alert1).not.toBeNull();
    expect(alert1?.text).toBe("WARNING: Harvester under attack!");
    expect(alert1?.tag).toBe("WARN");

    // Continuous attack 5 ticks later (0.5s) -> should be throttled
    const alert2 = logger.processUnderAttack(105, harvester);
    expect(alert2).toBeNull();

    // Attack 40 ticks later (4s) -> should trigger again
    const alert3 = logger.processUnderAttack(140, harvester);
    expect(alert3).not.toBeNull();
  });

  it("logs HQ and building under-attack as danger alert", () => {
    const logger = new IntelLogger();
    const hq: DiffEntity = { id: 1, kind: "Hq", owner: 0, x: 10, y: 10, hp: 1400, maxHp: 1500 };
    const entry = logger.processUnderAttack(50, hq);
    expect(entry?.text).toBe("ALERT: Base HQ under attack!");
    expect(entry?.level).toBe("danger");
    expect(entry?.tag).toBe("ALERT");
  });

  it("alerts for airfields under attack", () => {
    const logger = new IntelLogger();
    const airfield: DiffEntity = { id: 7, kind: "Airfield", owner: 0, x: 20, y: 20, hp: 180, maxHp: 250 };
    const entry = logger.processUnderAttack(60, airfield);
    expect(entry?.text).toBe("ALERT: Airfield under fire!");
    expect(entry?.level).toBe("danger");
    expect(entry?.tag).toBe("ALERT");
  });

  it("logs airfield destruction as critical friendly loss and enemy kill", () => {
    const logger = new IntelLogger();
    const friendly = logger.processEntityDestroyed(70, { id: 7, kind: "Airfield", owner: 0 });
    expect(friendly.text).toBe("CRITICAL: Airfield destroyed!");
    expect(friendly.level).toBe("danger");

    const enemy = logger.processEntityDestroyed(80, { id: 27, kind: "Airfield", owner: 1 });
    expect(enemy.text).toBe("Enemy Airfield destroyed!");
    expect(enemy.level).toBe("kill");
  });

  it("logs friendly casualties and hostile neutralizations", () => {
    const logger = new IntelLogger();

    // Friendly tank lost
    const friendlyLoss = logger.processEntityDestroyed(120, { id: 10, kind: "Tank", owner: 0 });
    expect(friendlyLoss.text).toBe("Unit lost: Tank");
    expect(friendlyLoss.level).toBe("danger");
    expect(friendlyLoss.tag).toBe("LOST");

    // Friendly refinery destroyed
    const buildingLoss = logger.processEntityDestroyed(130, { id: 2, kind: "Refinery", owner: 0 });
    expect(buildingLoss.text).toBe("CRITICAL: Refinery destroyed!");
    expect(buildingLoss.level).toBe("danger");

    // Enemy turret neutralized
    const enemyLoss = logger.processEntityDestroyed(140, { id: 25, kind: "Turret", owner: 1 });
    expect(enemyLoss.text).toBe("Enemy Turret destroyed!");
    expect(enemyLoss.level).toBe("kill");
    expect(enemyLoss.tag).toBe("KILL");

    // Enemy tank neutralized
    const hostileKill = logger.processEntityDestroyed(150, { id: 26, kind: "Tank", owner: 1 });
    expect(hostileKill.text).toBe("Hostile neutralized: Tank");
    expect(hostileKill.level).toBe("kill");
  });

  it("warns about low power with debouncing", () => {
    const logger = new IntelLogger();
    // Consumed 60, Produced 50 -> Low power alert
    const alert1 = logger.processPowerStatus(100, 50, 60);
    expect(alert1).not.toBeNull();
    expect(alert1?.tag).toBe("POWER");
    expect(alert1?.text).toContain("LOW POWER WARNING");

    // 10 ticks later -> throttled
    const alert2 = logger.processPowerStatus(110, 50, 60);
    expect(alert2).toBeNull();

    // Normal power -> no alert
    const alert3 = logger.processPowerStatus(250, 150, 60);
    expect(alert3).toBeNull();
  });
});
