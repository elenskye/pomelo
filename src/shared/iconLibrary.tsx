// The icon library: every place where I pick an icon for something of mine (a 念念不忘 tag, a 一步一步 group or goal,
// an icon dropped into a text) picks from here — all the UI icons drawn in visual-atelier (shared/icons.tsx, synced
// to src/vendor/pomelo/marks/icons/), in groups. No emoji.
// A picked icon is kept as `i:<key>`. The key is the icon's file name, except the 48 keys 念念不忘 has kept since
// before the icons were drawn (LEGACY: `pin` is the map pin, `notebook` the notebook with a pen…) — those stay as
// they are, and the two files they shadow get their own key (PUSHPIN, NOTEBOOK). An older value that isn't `i:…`
// is an emoji typed by hand: still shown, no longer offered.
// In a text an icon is written `:pm-<key>:` (shared/editor draws it in place; `:name:` without `pm-` is the
// website's own content icon, which only the website draws).
import { useMemo, useState, type CSSProperties } from "react";
import { Hash, Shapes } from "./icons";
import { markSvg } from "./marks";
import { Modal } from "./Modal";
import { Button } from "./ui";

/** Keys kept from before (key → file). */
const LEGACY: Record<string, string> = {
  notebook: "notebook-pen", pen: "pen-line", message: "message-circle", citrus: "pomelo", flower: "flower-2", pin: "map-pin",
  graduation: "graduation-cap", film: "clapperboard", game: "gamepad-2", food: "utensils", health: "heart-pulse", calendar: "calendar-days",
};
/** Files whose own name is a legacy key for another picture. */
const RENAMED: Record<string, string> = { pin: "pushpin", notebook: "notebook-plain" };

const GROUPS: [string, string[]][] = [
  ["写和读", ["book-open", "library", "library-big", "notebook", "notebook-plain", "pen", "pencil", "feather", "square-pen", "scroll-text", "file-text", "quote", "languages", "graduation", "lightbulb", "brain", "message", "message-square", "message-circle-question"]],
  ["记号", ["bookmark", "tag", "tags", "target", "star", "sparkle", "sparkles", "flame", "heart", "pushpin", "pin", "hash", "paperclip", "link-2", "lock", "key-round", "fingerprint", "eye", "bell-ring", "triangle-alert", "circle-question-mark", "trophy", "puzzle", "shapes"]],
  ["自然和时间", ["citrus", "sprout", "flower", "leaf", "moon", "sun", "cloud", "mountain", "calendar", "calendar-heart", "calendar-check", "alarm-clock", "hourglass", "history", "time-travel", "repeat"]],
  ["生活", ["house", "coffee", "food", "shopping-bag", "gift", "shirt", "cat", "smile", "user", "users", "health", "stethoscope", "dumbbell", "footprints", "luggage", "plane", "globe", "compass"]],
  ["兴趣", ["film", "tv", "music", "headphones", "palette", "game", "camera", "image-plus"]],
  ["工作和工具", ["briefcase", "wallet", "trending-up", "file-user", "code", "wrench", "keyboard", "hard-drive", "database-backup", "brush-cleaning", "settings", "settings-2", "sliders-horizontal", "wand-sparkles", "scan-text", "search", "send", "archive", "folder", "folder-open", "list-checks", "layout-grid"]],
];

const FILES = import.meta.glob<string>("/src/vendor/pomelo/marks/icons/*.svg", { query: "?raw", import: "default", eager: true });
const ALL_FILES = Object.keys(FILES).map((p) => p.replace(/^.*\//, "").replace(/\.svg$/, "")).sort();
const fileOf = (key: string): string | null => {
  const file = LEGACY[key] ?? Object.entries(RENAMED).find(([, k]) => k === key)?.[0] ?? (key in RENAMED ? null : key);
  return file && markSvg(`icons/${file}`) ? file : null;
};
const keyOfFile = (file: string) => RENAMED[file] ?? file;

type Entry = { key: string; label: string; body: string };
const entry = (key: string): Entry | null => {
  const file = fileOf(key);
  const svg = file ? markSvg(`icons/${file}`) : null;
  if (!svg) return null;
  return { key, label: /aria-label="([^"]*)"/.exec(svg)?.[1] ?? key, body: svg.replace(/^<svg[^>]*>/, "").replace(/<\/svg>\s*$/, "") };
};

/** The library in the picker's order: the groups above, then every drawn icon not named in them (方向、增删改… and
 *  whatever a later sync brings), so nothing drawn is missing. */
export const LIBRARY: { name: string; icons: Entry[] }[] = (() => {
  const seen = new Set<string>();
  const groups = GROUPS.map(([name, keys]) => ({
    name,
    icons: keys.flatMap((k) => {
      const e = entry(k);
      const file = fileOf(k);
      if (!e || !file || seen.has(file)) return [];
      seen.add(file);
      return [e];
    }),
  }));
  const rest = ALL_FILES.filter((f) => !seen.has(f)).flatMap((f) => entry(keyOfFile(f)) ?? []);
  return [...groups, { name: "符号", icons: rest }].filter((g) => g.icons.length > 0);
})();
const BY_KEY = new Map(LIBRARY.flatMap((g) => g.icons).map((e) => [e.key, e]));
// a legacy key whose picture is listed under another key still draws
for (const k of Object.keys(LEGACY)) if (!BY_KEY.has(k)) { const e = entry(k); if (e) BY_KEY.set(k, e); }

/** The library icon of a key (`pin`, `book-open`); null when there is none. */
export const libraryIcon = (key: string): Entry | null => BY_KEY.get(key) ?? null;

/** One library icon by key, following the text colour. */
export function LibIcon(props: { name: string; size?: number; className?: string; style?: CSSProperties }) {
  const e = BY_KEY.get(props.name);
  const size = props.size ?? 16;
  if (!e) return null;
  return <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden className={`shrink-0 ${props.className ?? ""}`} style={props.style} dangerouslySetInnerHTML={{ __html: e.body }} />;
}

/** A kept icon (`i:<key>`, or an emoji from before) in a small slot; `fallback` (a muted # when not given) without one. */
export function StoredIcon(props: { icon: string | null | undefined; size?: number; color?: string; fallback?: React.ReactNode }) {
  const size = props.size ?? 14;
  const key = props.icon?.startsWith("i:") ? props.icon.slice(2) : null;
  if (key && BY_KEY.has(key)) return <LibIcon name={key} size={size} style={props.color ? { color: props.color } : undefined} />;
  if (props.icon && !key) return <span className="shrink-0 leading-none" style={{ fontSize: size - 1 }}>{props.icon}</span>;
  return props.fallback === undefined ? <Hash size={size - 1} className="shrink-0 text-muted-foreground" /> : <>{props.fallback}</>;
}

/** The grid of the whole library; `onPick` gets a key. */
export function IconGrid(props: { current?: string | null; color?: string; onPick: (key: string) => void; size?: number; cols?: number }) {
  return (
    <div className="-m-1 max-h-[46vh] space-y-2 overflow-y-auto p-1">
      {LIBRARY.map((g) => (
        <div key={g.name}>
          <div className="px-1 pb-0.5 text-xs text-muted-foreground">{g.name}</div>
          <div className="grid gap-0.5" style={{ gridTemplateColumns: `repeat(${props.cols ?? 10}, minmax(0, 1fr))` }}>
            {g.icons.map((e) => {
              const on = props.current === e.key;
              return (
                <button
                  key={e.key}
                  type="button"
                  title={e.label}
                  aria-label={e.label}
                  aria-pressed={on}
                  onClick={() => props.onPick(e.key)}
                  className={`icon-cell grid aspect-square place-items-center rounded-control outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "on" : ""}`}
                >
                  <LibIcon name={e.key} size={props.size ?? 18} style={props.color ? { color: props.color } : undefined} />
                </button>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}

/** Pick an icon for something: a click picks and closes; 「不用图标」 takes it off. `icon` / `onPick` are the kept
 *  form (`i:<key>` or null). */
export function IconDialog(props: { title: string; icon: string | null; color?: string; onPick: (icon: string | null) => void; onClose: () => void }) {
  const current = props.icon?.startsWith("i:") ? props.icon.slice(2) : null;
  return (
    <Modal
      title={props.title}
      onClose={props.onClose}
      error={null}
      wide
      footerLeft={
        props.icon && (
          <Button variant="ghost" onClick={() => { props.onPick(null); props.onClose(); }}>
            不用图标
          </Button>
        )
      }
    >
      <IconGrid current={current} color={props.color} onPick={(k) => { props.onPick(`i:${k}`); props.onClose(); }} />
    </Modal>
  );
}

/** The icon of a thing being edited in a dialog: a button showing it, opening the library over the dialog. */
export function IconField(props: { icon: string | null; onChange: (icon: string | null) => void; color?: string; label: string }) {
  const [open, setOpen] = useState(false);
  const seed = useMemo(() => 1460 + (props.label.length % 9), [props.label]);
  return (
    <>
      <button type="button" aria-label={props.label} onClick={() => setOpen(true)} className="pm-field pm-compact icon-field shrink-0" data-hand="field" data-seed={seed}>
        <StoredIcon icon={props.icon} size={18} color={props.color} fallback={<Shapes size={17} className="text-muted-foreground opacity-60" />} />
      </button>
      {open && <IconDialog title={props.label} icon={props.icon} color={props.color} onPick={props.onChange} onClose={() => setOpen(false)} />}
    </>
  );
}
