// Server-side caps for command batches (crucible-server/src/ws.rs):
//   MAX_COMMANDS_PER_MESSAGE      = 8
//   MAX_MOVE_GROUP_UNITS          = 32 ids per single MoveGroup/Attack command
//   MAX_MOVE_GROUP_UNITS_PER_BATCH= 64 ids TOTAL across all commands in one
//                                   message — an oversized batch is dropped
//                                   whole, with no feedback to the client.
// So a large selection must be split into ≤32-id commands packed at most two
// per message (2 × 32 = 64). Exported pure so it can be unit-tested.

export const MAX_GROUP_IDS = 32;
export const MAX_IDS_PER_MESSAGE = 64;

/** Build the command messages for `units`, respecting every server cap.
 *  `make` receives at most {@link MAX_GROUP_IDS} ids per call. */
export function packGroupMessages<T>(
  units: number[],
  make: (ids: number[]) => T,
): T[][] {
  const messages: T[][] = [];
  for (let i = 0; i < units.length; i += MAX_IDS_PER_MESSAGE) {
    const slice = units.slice(i, i + MAX_IDS_PER_MESSAGE);
    const cmds: T[] = [];
    for (let j = 0; j < slice.length; j += MAX_GROUP_IDS) {
      cmds.push(make(slice.slice(j, j + MAX_GROUP_IDS)));
    }
    messages.push(cmds);
  }
  return messages;
}
