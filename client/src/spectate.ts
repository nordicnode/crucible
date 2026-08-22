// Spectate screen: list stored replays, load one, and step through it
// client-side via the wasm replay shim (full state, no fog). The wasm runs the
// exact same sim as the server, so a replay is reproduced byte-for-byte.

import { Renderer } from "./renderer";
import { World } from "./world";
import { applyFrame, applyMeta, type ReplayMeta } from "./snapshot";
import { frame as wasmFrame, meta as wasmMeta } from "./wasm/loader";

const TICKS_PER_SEC = 10;
const SPEEDS = [1, 2, 4, 8];

interface ReplaySummary {
  id: number;
  map_seed: number;
  p1_type: string;
  p2_type: string;
  result: string;
  duration_ticks: number;
  created_at: number;
}

function el<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

function fmtClock(tick: number): string {
  const s = Math.floor(tick / TICKS_PER_SEC);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function show(id: string): void {
  el(id).classList.remove("hidden");
}
function hide(id: string): void {
  el(id).classList.add("hidden");
}

class Spectate {
  active = false;
  readonly renderer = new Renderer();
  readonly world = new World();
  private replayJson: string | null = null;
  private meta: ReplayMeta | null = null;
  private tick = 0;
  private renderedTick = -1;
  private duration = 0;
  private playing = false;
  private speedIndex = 0;
  private lastTime = 0;
  private loading = false;
  private pendingTick: number | null = null;
  private ore0 = 0;
  private ore1 = 0;

  get currentTick(): number {
    return this.tick;
  }
  get score0(): number {
    return this.ore0;
  }
  get score1(): number {
    return this.ore1;
  }

  init(): void {
    document.getElementById("open-spectate")?.addEventListener("click", () => void this.open());
    el("spectate-list-back").addEventListener("click", () => this.close());
    el("sp-play").addEventListener("click", () => this.toggle());
    el("sp-speed").addEventListener("click", () => this.cycleSpeed());
    el("sp-close").addEventListener("click", () => this.close());
    el("sp-scrub").addEventListener("input", () => {
      this.tick = Number(el<HTMLInputElement>("sp-scrub").value);
      this.updateClock();
      void this.requestFrame(this.tick);
    });
  }

  // --- screen transitions -------------------------------------------------

  async open(): Promise<void> {
    hide("lobby");
    hide("result");
    hide("dashboard");
    hide("museum");
    hide("sidebar");
    hide("log");
    hide("topbar");
    hide("spectate-bar");
    show("overlay");
    show("spectate-list");
    this.active = false;
    this.setStatus("loading replays…");
    this.renderList([]);
    try {
      const res = await fetch("/api/replays");
      if (!res.ok) throw new Error(`replays: ${res.status}`);
      const data = (await res.json()) as { matches: ReplaySummary[] };
      this.renderList(data.matches ?? []);
    } catch (e) {
      this.renderList([], `error: ${String(e)}`);
    }
  }

  async loadReplay(id: number): Promise<void> {
    this.setStatus("loading replay #" + id + "…");
    try {
      const res = await fetch(`/api/replay/${id}`);
      if (!res.ok) throw new Error(`replay: ${res.status}`);
      const data = (await res.json()) as { replay: string };
      this.replayJson = data.replay;
      this.meta = await wasmMeta(data.replay);
      applyMeta(this.world, this.meta);
      this.duration = Math.max(1, this.meta.duration_ticks);

      const scrub = el<HTMLInputElement>("sp-scrub");
      scrub.max = String(this.duration);
      scrub.value = "0";

      // Camera on player 0's HQ, like a live match start.
      const hq = this.meta.hq_tiles[0];
      this.renderer.camera.centerOn(
        hq[0] + 0.5,
        hq[1] + 0.5,
        window.innerWidth,
        window.innerHeight,
        18,
      );

      hide("overlay");
      hide("spectate-list");
      show("spectate-bar");
      this.active = true;
      this.playing = false;
      this.tick = 0;
      this.renderedTick = -1;
      this.lastTime = performance.now();
      await this.requestFrame(0);
    } catch (e) {
      this.setStatus(`error: ${String(e)}`);
    }
  }

  close(): void {
    this.active = false;
    this.playing = false;
    this.replayJson = null;
    this.meta = null;
    hide("spectate-bar");
    hide("spectate-list");
    hide("dashboard");
    show("overlay");
    show("lobby");
  }

  // --- controls -----------------------------------------------------------

  toggle(): void {
    if (this.replayJson == null) return;
    this.playing = !this.playing;
    this.lastTime = performance.now();
    el("sp-play").textContent = this.playing ? "PAUSE" : "PLAY";
    if (this.playing && this.tick >= this.duration) this.tick = 0;
  }

  cycleSpeed(): void {
    this.speedIndex = (this.speedIndex + 1) % SPEEDS.length;
    el("sp-speed").textContent = `${SPEEDS[this.speedIndex]}×`;
  }

  // --- per-frame ----------------------------------------------------------

  draw(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    const now = performance.now();
    const dt = Math.min(0.25, (now - this.lastTime) / 1000);
    this.lastTime = now;

    // Glide display positions toward the latest frame's authoritative ones so
    // playback is smooth between the replay's 10 Hz frames (this also drives
    // unit heading rotation and turret aiming, like a live match).
    this.world.advance(dt * 1000);

    if (this.playing && this.replayJson != null) {
      this.tick += dt * TICKS_PER_SEC * SPEEDS[this.speedIndex];
      if (this.tick >= this.duration) {
        this.tick = this.duration;
        this.playing = false;
        el("sp-play").textContent = "PLAY";
      }
      const t = Math.floor(this.tick);
      if (t !== this.renderedTick) {
        this.updateClock();
        void this.requestFrame(t);
      }
    }

    this.renderer.draw(ctx, this.world, new Set(), w, h);
    const scrub = el<HTMLInputElement>("sp-scrub");
    if (!this.loading) scrub.value = String(Math.floor(this.tick));
  }

  private async requestFrame(t: number): Promise<void> {
    if (this.loading) {
      this.pendingTick = t;
      return;
    }
    if (this.replayJson == null) return;
    this.loading = true;
    try {
      const f = await wasmFrame(this.replayJson, t);
      applyFrame(this.world, f);
      this.renderedTick = f.tick;
      this.ore0 = f.ore0;
      this.ore1 = f.ore1;
      this.updateHud();
    } catch (e) {
      // A corrupt/legacy replay must degrade to a visible error, never crash
      // the page (the wasm shim returns errors instead of panicking).
      this.playing = false;
      this.active = false;
      this.replayJson = null;
      hide("spectate-bar");
      show("overlay");
      show("spectate-list");
      this.setStatus(`replay error at tick ${t}: ${String(e)}`);
    } finally {
      this.loading = false;
    }
    if (this.pendingTick != null) {
      const p = this.pendingTick;
      this.pendingTick = null;
      void this.requestFrame(p);
    }
  }

  private updateClock(): void {
    el("sp-clock").textContent = `${fmtClock(this.tick)} / ${fmtClock(this.duration)}`;
  }

  private updateHud(): void {
    this.updateClock();
    const won = this.world.result?.winner;
    const result =
      won == null ? "" : won === 0 ? " — P0 wins" : " — P1 wins";
    el("sp-score").textContent = `P0 ${this.ore0} · P1 ${this.ore1}${result}`;
  }

  // --- list rendering -----------------------------------------------------

  private setStatus(s: string): void {
    el("spectate-status").textContent = s;
  }

  private renderList(matches: ReplaySummary[], error?: string): void {
    this.setStatus(error ?? (matches.length === 0 ? "No replays yet — play a match first." : ""));
    const body = el("spectate-list-body");
    body.innerHTML = "";
    for (const m of matches) {
      const row = document.createElement("button");
      row.className = "btn spectate-row";
      const when = new Date(m.created_at * 1000).toLocaleString();
      row.textContent =
        `#${m.id} ${m.p1_type} vs ${m.p2_type} · ${fmtClock(m.duration_ticks)} · ${when}`;
      row.addEventListener("click", () => void this.loadReplay(m.id));
      body.appendChild(row);
    }
  }
}

export const spectate = new Spectate();
