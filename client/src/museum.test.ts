import { describe, expect, it } from "vitest";
import {
  clampPage,
  formatGauntlet,
  formatReign,
  pageLabel,
  type MuseumGauntlet,
} from "./museum";

describe("museum formatReign", () => {
  it("formats reign lengths as human durations", () => {
    expect(formatReign(null, true)).toBe("still reigning");
    expect(formatReign(45, false)).toBe("45s");
    expect(formatReign(60, false)).toBe("1m");
    expect(formatReign(3600, false)).toBe("1h 0m");
    expect(formatReign(3600 + 30 * 60, false)).toBe("1h 30m");
    expect(formatReign(2 * 86400 + 7 * 3600, false)).toBe("2d 7h");
    expect(formatReign(null, false)).toBe("—");
  });
});

describe("museum formatGauntlet", () => {
  const g: MuseumGauntlet = { champion_wins: 14, champion_total: 20, historical_wins: 8, historical_total: 16 };
  it("joins champion and historical records", () => {
    expect(formatGauntlet(g)).toBe("14/20 vs champion · 8/16 vs historical");
  });
  it("omits the historical half when none was played", () => {
    expect(formatGauntlet({ champion_wins: 1, champion_total: 2, historical_wins: 0, historical_total: 0 })).toBe(
      "1/2 vs champion",
    );
  });
  it("returns a dash when unrecorded", () => {
    expect(formatGauntlet(null)).toBe("—");
    expect(formatGauntlet({ champion_wins: 0, champion_total: 0, historical_wins: 0, historical_total: 0 })).toBe("—");
  });
});

describe("museum pagination", () => {
  it("clamps pages into [0, last]", () => {
    expect(clampPage(10, 0, 25)).toBe(0);
    expect(clampPage(10, 5, 25)).toBe(0);
    expect(clampPage(100, 9, 25)).toBe(3);
    expect(clampPage(100, 0, 25)).toBe(0);
    expect(clampPage(0, 3, 25)).toBe(0);
    expect(clampPage(100, -2, 25)).toBe(0);
  });

  it("labels pages with totals", () => {
    expect(pageLabel(0, 25, 25)).toBe("PAGE 1 / 1 · 25 CHAMPIONS");
    expect(pageLabel(1, 100, 25)).toBe("PAGE 2 / 4 · 100 CHAMPIONS");
    expect(pageLabel(0, 0, 25)).toBe("PAGE 1 / 1 · 0 CHAMPIONS");
  });
});
