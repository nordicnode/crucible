import { describe, expect, it } from "vitest";
import { MAX_GROUP_IDS, MAX_IDS_PER_MESSAGE, packGroupMessages } from "./groupCommands";

// Mirror of the server's command_batch_is_bounded (crates/crucible-server/src/ws.rs):
// a message is legal iff it has ≤8 commands, each command carries ≤32 unit
// ids, and the message's total id count is ≤64.
const idsOf = (cmd: { units: number[] }): number => cmd.units.length;
const isLegalMessage = (cmds: { units: number[] }[]): boolean =>
  cmds.length <= 8 &&
  cmds.every((c) => idsOf(c) <= MAX_GROUP_IDS) &&
  cmds.reduce((sum, c) => sum + idsOf(c), 0) <= MAX_IDS_PER_MESSAGE;

const make = (ids: number[]): { units: number[] } => ({ units: [...ids] });
const seq = (n: number): number[] => Array.from({ length: n }, (_, i) => i + 1);

describe("packGroupMessages", () => {
  it("keeps a small selection as one single-command message", () => {
    const msgs = packGroupMessages(seq(5), make);
    expect(msgs).toEqual([[{ units: [1, 2, 3, 4, 5] }]]);
    expect(msgs.every(isLegalMessage)).toBe(true);
  });

  it("emits exactly one message at the per-message boundary (64 ids)", () => {
    const msgs = packGroupMessages(seq(64), make);
    expect(msgs).toHaveLength(1);
    expect(isLegalMessage(msgs[0])).toBe(true);
    const all = msgs[0].flatMap((c) => c.units);
    expect(all).toEqual(seq(64));
  });

  it("splits across messages once the batch cap would be exceeded (65+)", () => {
    const msgs = packGroupMessages(seq(65), make);
    expect(msgs).toHaveLength(2);
    // Every message must pass the server's full boundedness check.
    expect(msgs.every(isLegalMessage)).toBe(true);
    // No unit lost, none duplicated, order preserved.
    expect(msgs.flatMap((m) => m.flatMap((c) => c.units))).toEqual(seq(65));
  });

  it("never emits an empty message and handles empty input", () => {
    expect(packGroupMessages([], make)).toEqual([]);
  });

  it("chunks each command at 32 ids", () => {
    const msgs = packGroupMessages(seq(96), make);
    for (const msg of msgs) {
      for (const cmd of msg) {
        expect(cmd.units.length).toBeLessThanOrEqual(MAX_GROUP_IDS);
      }
    }
    expect(msgs.flatMap((m) => m.flatMap((c) => c.units))).toEqual(seq(96));
  });

  it("handles selections far beyond any real army without dropping units", () => {
    const n = 500;
    const msgs = packGroupMessages(seq(n), make);
    expect(msgs.every(isLegalMessage)).toBe(true);
    expect(msgs.flatMap((m) => m.flatMap((c) => c.units))).toHaveLength(n);
  });
});
