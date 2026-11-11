// Atlas's designs: the frozen releases synced from visual-atelier (schemes/pomelo/atlas/releases/ →
// src/vendor/pomelo/atlas/<id>-<version>/design.js, never edited here). A release is one module that draws a whole
// 年度小报 from a run's 「呈现结果」. Every one ever adopted stays in the app: a run is always drawn with the design it
// was made for (its `design`, `<id>@<version>`), also after newer designs exist. Loaded only when a 小报 is opened.
// A release brings everything it needs (its pictures, the Design System's styles and painter as they were then).
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
};

const files = import.meta.glob<{ design: Design }>("../../vendor/pomelo/atlas/*/design.js");
/** `tabloid@1.0.0` → its loader. */
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

/** The design a run was made for; a key the app doesn't have (it shouldn't happen) falls back to the newest. */
export async function loadDesign(key: string): Promise<Design> {
  const load = loaders.get(key) ?? loaders.get(designKeys().at(-1) ?? "");
  if (!load) throw new Error("这个版本的柚儿园里没有 Atlas 的设计");
  return (await load()).design;
}

/** 「Tabloid 1.0.0」 */
export const designLabel = (key: string) => { const [id, v] = key.split("@"); return `${id.charAt(0).toUpperCase()}${id.slice(1)} ${v ?? ""}`.trim(); };
