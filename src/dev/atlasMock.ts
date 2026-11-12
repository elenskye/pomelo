// Atlas in the browser mock (?mock#/atlas): three made-up runs (atlasMock.json, from visual-atelier's sample year by
// scripts/atlas-mock.mjs). 立即运行 / 补做 "writes" for three seconds and adds a copy; 删除 removes one.
// `&atlas=envelope` pretends it's the new year's first week (the envelope on 首页).
import runs from "./atlasMock.json";

type Run = { id: number; year: string; until: string | null; createdAt: string; design: string; modelName: string; letter: string; snapshot: unknown; scenes: { key: string }[] | null; error: string | null };
const all = runs as unknown as Run[];
let seq = 10;
let writing: string | null = null;
const envelope = () => new URLSearchParams(location.search).get("atlas") === "envelope";
const today = () => (envelope() ? "2026-01-03" : "2026-11-10");

export const atlasHandlers: Record<string, (a: Record<string, unknown>) => unknown> = {
  atlas_overview: () => ({
    entries: [...all]
      .sort((a, b) => b.year.localeCompare(a.year) || b.id - a.id)
      .map((r) => ({ id: r.id, year: r.year, until: r.until, createdAt: r.createdAt, design: r.design, scenes: r.scenes?.length ?? 0, error: r.error, modelName: r.modelName, sessionId: null })),
    // enough years for the row to scroll sideways
    years: [{ year: "2026", records: 956 }, { year: "2025", records: 700 }, { year: "2024", records: 212 }, { year: "2023", records: 38 }, ...["2022", "2021", "2020", "2019", "2018", "2017", "2016"].map((year, i) => ({ year, records: 30 - i * 4 }))],
    writing,
    today: today(),
    design: "tabloid@1.0.3",
    envelope: envelope() ? (all.find((r) => r.year === "2025" && !r.until)?.id ?? null) : null,
  }),
  atlas_get: (a) => {
    const r = all.find((x) => x.id === a.id);
    if (!r) throw "找不到这一份年度小报，可能已经被删除了";
    return r;
  },
  atlas_delete: (a) => {
    const i = all.findIndex((x) => x.id === a.id);
    if (i < 0) throw "找不到这一份年度小报，可能已经被删除了";
    all.splice(i, 1);
  },
  atlas_run: (a) => {
    if (writing) throw "有一个定时任务正在运行，等它写完再试。";
    const year = (a.year as string | null) ?? today().slice(0, 4);
    writing = year;
    setTimeout(() => {
      const from = all.find((r) => r.year === year) ?? all[0] ?? (runs as unknown as Run[])[0];
      const copy = JSON.parse(JSON.stringify(from).replaceAll(from.year, year)) as Run;
      all.push({ ...copy, id: seq++, year, design: "tabloid@1.0.3", until: year === today().slice(0, 4) ? today() : null, createdAt: new Date().toISOString() });
      writing = null;
    }, 3000);
  },
};
