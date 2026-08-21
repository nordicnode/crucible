// The Museum Archive: a dedicated, paginated view of every champion ever
// crowned. Reads /api/museum (server-side paging + sorting); renders a dense
// per-champion table with era, Elo, reign length, gauntlet record, and
// lineage. Pure formatters live here so they are unit-testable without a DOM.

export interface MuseumGauntlet {
  champion_wins: number;
  champion_total: number;
  historical_wins: number;
  historical_total: number;
}

export interface MuseumChampion {
  id: number;
  genome_id: number;
  generation: number;
  crowned_at: number;
  dethroned_at: number | null;
  reigning: boolean;
  era: string | null;
  elo: number | null;
  reign_secs: number | null;
  parent_genome_id: number | null;
  gauntlet: MuseumGauntlet | null;
}

export interface MuseumPage {
  total: number;
  page: number;
  page_size: number;
  sort: string;
  champions: MuseumChampion[];
}

export const PAGE_SIZE = 25;
export const SORTS = [
  { value: "crowned_desc", label: "Newest first" },
  { value: "crowned_asc", label: "Oldest first" },
  { value: "generation_desc", label: "Generation" },
  { value: "elo_desc", label: "Highest Elo" },
] as const;

export const MAX_PAGES_SHOWN = 1000;

// --- Pure helpers (unit-tested) -------------------------------------------

/** Human-readable reign length, e.g. "2d 7h", "45m", or "still reigning". */
export function formatReign(secs: number | null, reigning: boolean): string {
  if (reigning) return "still reigning";
  if (secs == null || !Number.isFinite(secs) || secs < 0) return "—";
  const s = Math.floor(secs);
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${s}s`;
}

/** "14/20 vs champion · 8/16 vs historical", or a dash when unrecorded. */
export function formatGauntlet(g: MuseumGauntlet | null): string {
  if (!g || g.champion_total === 0) return "—";
  const champ = `${g.champion_wins}/${g.champion_total} vs champion`;
  if (g.historical_total > 0) {
    return `${champ} · ${g.historical_wins}/${g.historical_total} vs historical`;
  }
  return champ;
}

/** Clamp a requested page (0-based) into [0, lastPage]; 0 when empty. */
export function clampPage(total: number, page: number, pageSize: number): number {
  const safe = Math.max(1, pageSize);
  const last = Math.max(0, Math.ceil(total / safe) - 1);
  if (!Number.isFinite(page)) return 0;
  return Math.min(Math.max(0, Math.floor(page)), last);
}

/** "PAGE 2 / 9 · 225 CHAMPIONS" footer text. */
export function pageLabel(page: number, total: number, pageSize: number): string {
  const pages = Math.max(1, Math.ceil(total / Math.max(1, pageSize)));
  return `PAGE ${page + 1} / ${pages} · ${total} CHAMPIONS`;
}

// --- DOM view --------------------------------------------------------------

const el = (id: string): HTMLElement | null => document.getElementById(id);

function fmtDate(unix: number): string {
  return new Date(unix * 1000).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export class MuseumView {
  private page = 0;
  private sort = "crowned_desc";
  private total = 0;

  open(): void {
    el("lobby")?.classList.add("hidden");
    el("result")?.classList.add("hidden");
    el("dashboard")?.classList.add("hidden");
    el("spectate-list")?.classList.add("hidden");
    el("sidebar")?.classList.add("hidden");
    el("log")?.classList.add("hidden");
    el("overlay")?.classList.remove("hidden");
    el("museum")?.classList.remove("hidden");
    this.page = 0;
    void this.load();
  }

  close(): void {
    el("museum")?.classList.add("hidden");
    el("overlay")?.classList.remove("hidden");
    el("lobby")?.classList.remove("hidden");
  }

  private async load(): Promise<void> {
    const body = el("museum-body");
    const status = el("museum-status");
    if (body) body.innerHTML = "";
    if (status) status.textContent = "loading archive…";
    try {
      const res = await fetch(`/api/museum?page=${this.page}&page_size=${PAGE_SIZE}&sort=${this.sort}`);
      if (!res.ok) throw new Error(`museum: ${res.status}`);
      const data = (await res.json()) as MuseumPage;
      this.total = data.total;
      this.page = data.page;
      this.render(data);
      if (status) status.textContent = "";
    } catch (e) {
      if (status) status.textContent = `error: ${String(e)}`;
    }
  }

  private render(data: MuseumPage): void {
    const body = el("museum-body");
    if (!body) return;
    if (data.champions.length === 0) {
      body.innerHTML = '<div class="muted">The museum is empty — no champion has been crowned yet.</div>';
    } else {
      const rows = data.champions
        .map((c) => {
          const elo = c.elo == null ? "—" : String(Math.round(c.elo));
          const era = c.era ?? "—";
          const badge = c.reigning
            ? '<span style="color:var(--amber); font-weight:700;">👑 REIGNING</span>'
            : '<span class="muted">dethroned</span>';
          const parent =
            c.parent_genome_id == null ? "—" : `<a href="#" data-parent="${c.parent_genome_id}">#${c.parent_genome_id}</a>`;
          return `<tr>
            <td>#${c.genome_id}</td>
            <td>gen ${c.generation}</td>
            <td>${era}</td>
            <td>Elo ${elo}</td>
            <td>${fmtDate(c.crowned_at)}</td>
            <td>${formatReign(c.reign_secs, c.reigning)}</td>
            <td class="muted">${formatGauntlet(c.gauntlet)}</td>
            <td>${badge}</td>
            <td>${parent}</td>
          </tr>`;
        })
        .join("");
      body.innerHTML = `<table class="museum-table">
        <thead><tr><th>genome</th><th>generation</th><th>era</th><th>elo</th><th>crowned</th><th>reign</th><th>gauntlet</th><th></th><th>lineage parent</th></tr></thead>
        <tbody>${rows}</tbody>
      </table>`;
      // Lineage-parent links jump to that genome's champion row if present
      // on this page (a simple same-page anchor).
      body.querySelectorAll<HTMLAnchorElement>("a[data-parent]").forEach((a) => {
        a.addEventListener("click", (ev) => {
          ev.preventDefault();
          const target = Number(a.dataset.parent);
          const cell = [...body.querySelectorAll("tbody tr")].find((tr) =>
            tr.textContent?.includes(`#${target}`),
          );
          cell?.scrollIntoView({ block: "center" });
        });
      });
    }

    const totalEl = el("museum-total");
    const pagesEl = el("museum-pages");
    if (totalEl) totalEl.textContent = `${data.total} champion${data.total === 1 ? "" : "s"} archived`;
    if (pagesEl) pagesEl.textContent = pageLabel(data.page, data.total, data.page_size);
    const prev = el("museum-prev") as HTMLButtonElement | null;
    const next = el("museum-next") as HTMLButtonElement | null;
    if (prev) prev.disabled = data.page === 0;
    if (next) next.disabled = data.page + 1 >= Math.ceil(data.total / Math.max(1, data.page_size));
  }

  init(): void {
    el("open-museum")?.addEventListener("click", () => this.open());
    el("museum-back")?.addEventListener("click", () => this.close());
    el("museum-prev")?.addEventListener("click", () => {
      this.page = clampPage(this.total, this.page - 1, PAGE_SIZE);
      void this.load();
    });
    el("museum-next")?.addEventListener("click", () => {
      this.page = clampPage(this.total, this.page + 1, PAGE_SIZE);
      void this.load();
    });
    const sort = el("museum-sort") as HTMLSelectElement | null;
    if (sort) {
      for (const s of SORTS) {
        const opt = document.createElement("option");
        opt.value = s.value;
        opt.textContent = s.label;
        sort.appendChild(opt);
      }
      sort.value = this.sort;
      sort.addEventListener("change", () => {
        this.sort = sort.value;
        this.page = 0;
        void this.load();
      });
    }
  }
}

export const museum = new MuseumView();
