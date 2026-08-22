// Loader for the wasm replay engine. A committed pure-JS fallback module
// (crucible_client_wasm.js, see its header) keeps Vite/tsc green without a
// Rust toolchain; its replay calls throw a clear error until a real build
// (`npm run wasm`) overwrites it with the generated wasm-bindgen bindings.
// This module is imported lazily so the main bundle never pulls the wasm in
// until spectate/replay is actually used.

import init, {
  replay_frame,
  replay_meta,
  replay_result,
  replay_snapshot_json,
  sim_version,
} from "./crucible_client_wasm";
import wasmUrl from "./crucible_client_wasm_bg.wasm?url";

import type { ReplayFrame, ReplayMeta } from "../snapshot";

let ready: Promise<unknown> | null = null;

/** Instantiate the wasm module once. Safe to call repeatedly. */
export function wasmInit(): Promise<unknown> {
  if (!ready) ready = init(wasmUrl);
  return ready;
}

export async function meta(replayJson: string): Promise<ReplayMeta> {
  await wasmInit();
  return JSON.parse(replay_meta(replayJson)) as ReplayMeta;
}

export async function frame(replayJson: string, tick: number): Promise<ReplayFrame> {
  await wasmInit();
  return JSON.parse(replay_frame(replayJson, tick)) as ReplayFrame;
}

export async function result(replayJson: string): Promise<{
  winner: number | null;
  reason: string | null;
  duration_ticks: number;
  hash: number;
}> {
  await wasmInit();
  return JSON.parse(replay_result(replayJson));
}

export async function snapshotJson(replayJson: string, tick: number): Promise<unknown> {
  await wasmInit();
  return JSON.parse(replay_snapshot_json(replayJson, tick)) as unknown;
}

export async function version(): Promise<string> {
  await wasmInit();
  return sim_version();
}
