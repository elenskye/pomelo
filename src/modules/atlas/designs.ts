// Atlas's designs: the frozen releases synced from visual-atelier (schemes/pomelo/atlas/releases/ →
// src/vendor/pomelo/atlas/<id>-<version>/design.js, never edited here). A release is one module that draws a whole
// 年度小报 from a run's 「呈现结果」, and (1.0.2 on) the run's cover on Atlas's page. Loaded only when needed.
// A release brings everything it needs (its pictures, the Design System's styles and painter as they were then).
// Which releases count (visual-atelier schemes/pomelo/atlas/README.md, 2026-11-11): a patch replaces the earlier ones
// of its line (same first two numbers) — 1.0.1 → 1.0.2 leaves only 1.0.2 in the app —, a new minor or major is a new
// line and both stay. So a run keeps the design it was made for (`design`, `tabloid@1.0.0`) and is drawn with the
// newest release of that line the app has (`resolve`): 1.0.x runs with 1.0.2; a later 1.1.0 leaves them alone.
export type Hooks = { openRecord?: (key: string) => void; onClose?: () => void; onEscape?: () => void };
export type Mounted = { root: HTMLElement; set: (o: { palette?: string; theme?: string; reduce?: boolean }) => void; destroy: () => void };
export type Design = {
  id: string;
  name: string;
  version: string;
  css: string;
  /** Into an element of the frame's document (`frame()`). desk: the envelope first; backdrop: lays its own desk. */
  mount: (host: HTMLElement, raw: unknown, opts: { desk?: boolean; backdrop?: boolean; palette?: string; theme?: string; reduce?: boolean; hooks?: Hooks }) => Mounted;
  /** The document of the frame it is shown in (for `srcdoc`); its body is empty, waiting for `mount`. */
  frame: () => string;
  /** the envelope on 首页's desk (HTML; styled by the Design System's atlas-envelope part) */
  envelope: (year: number | string) => string;
  /** The run's cover on Atlas's page (1.0.2 on): HTML of a 168 × 218 printed cover with the year, the design's name and
   *  version, and 「截至某日」 when the year wasn't over. Inline styles only, put straight in the page. */
  cover?: (o: { year: number | string; until?: string | null }) => string;
};

const files = import.meta.glob<{ design: Design }>("../../vendor/pomelo/atlas/*/design.js");
/** `tabloid@1.0.2` → its loader. */
const loaders = new Map<string, () => Promise<{ design: Design }>>(
  Object.entries(files).map(([path, load]) => {
    const dir = path.split("/").slice(-2)[0];
    const at = dir.lastIndexOf("-");
    return [`${dir.slice(0, at)}@${dir.slice(at + 1)}`, load] as [string, () => Promise<{ design: Design }>];
  }),
);

const parts = (key: string) => key.split("@")[1]?.split(".").map(Number) ?? [];
/** The releases in the app, newest last (by version, then name). */
export const designKeys = () => [...loaders.keys()].sort((a, b) => { const [x, y] = [parts(a), parts(b)]; return x[0] - y[0] || x[1] - y[1] || x[2] - y[2] || a.localeCompare(b); });

/** The release a run is drawn with: the newest of its line (same design, same first two numbers); a line the app
 *  doesn't carry (it shouldn't happen) falls back to the design's newest release, then to the newest of all. */
export function resolve(key: string): string {
  const [id] = key.split("@");
  const [major, minor] = parts(key);
  const all = designKeys();
  const mine = all.filter((k) => k.split("@")[0] === id);
  const line = mine.filter((k) => parts(k)[0] === major && parts(k)[1] === minor);
  return line.at(-1) ?? mine.at(-1) ?? all.at(-1) ?? key;
}

/** What a release says about itself (its design.json): when it was frozen and what it is. */
type Meta = { id: string; name: string; version: string; frozen: string | null; note: string };
const metas = import.meta.glob<Meta>("../../vendor/pomelo/atlas/*/design.json", { eager: true, import: "default" });
/** Every release in the app, newest first (Atlas's 样式 view). */
export function designList(): (Meta & { key: string })[] {
  const byKey = new Map(Object.values(metas).map((m) => [`${m.id}@${m.version}`, m]));
  return designKeys()
    .reverse()
    .map((key) => {
      const [id, version] = key.split("@");
      return { id, name: id.charAt(0).toUpperCase() + id.slice(1), version: version ?? "", frozen: null, note: "", ...byKey.get(key), key };
    });
}

const loaded = new Map<string, Promise<Design>>();
/** The design a run is drawn with (`resolve`), loaded once. */
export function loadDesign(key: string): Promise<Design> {
  const at = resolve(key);
  const load = loaders.get(at);
  if (!load) return Promise.reject(new Error("这个版本的柚儿园里没有 Atlas 的设计"));
  let p = loaded.get(at);
  if (!p) {
    p = load().then((m) => m.design);
    p.catch(() => loaded.delete(at));
    loaded.set(at, p);
  }
  return p;
}

/** 「Tabloid 1.0.2」: the release a run is drawn with, not the one it was first made for. */
export const designLabel = (key: string) => { const [id, v] = resolve(key).split("@"); return `${id.charAt(0).toUpperCase()}${id.slice(1)} ${v ?? ""}`.trim(); };
