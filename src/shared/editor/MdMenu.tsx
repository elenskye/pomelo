// The list a right-click opens in every box where I write Markdown (LiveText: 念念不忘, 速记小窗, 呦呦漫游, the
// prompts): the formats that box's flavor knows (加粗, 列表, 标题…), then the two icon libraries —
// 柚儿园图标 (Pomelo's own: goes in as `:pm-<key>:` and is drawn in place, here and wherever Pomelo shows the text)
// and 网站图标 (the website's content icons, synced as src/vendor/pomelo/content-icons.json: goes in as `:name:`,
// which only the website draws — written anywhere else in Pomelo it stays those letters).
// The Design System's menu (a strip of paper, pm-menu); a library opens in the same strip. Esc, a click outside or
// a choice closes it; the box keeps its focus and selection.
// Formats of one `group` (见字如面, Markdown: 文字 / 段落 / 插入…) fold into one row that opens a second strip beside
// it — on the pointer coming to rest on it, or a click (useSideStrip) — so a long list stays short.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import list from "@/vendor/pomelo/content-icons.json";
import { ChevronLeft, ChevronRight } from "@/shared/icons";
import { IconGrid, libraryIcon } from "@/shared/iconLibrary";
import { iconToken, setIconDrawer } from "./inlineIcon";
import type { Format } from "./LiveText";
import "./md-menu.css";

// the editors draw `:pm-<key>:` with the library's drawings
setIconDrawer((key) => libraryIcon(key)?.body ?? null);

type ContentIcon = { name: string; label: { zh: string; en: string }; svg: string };
const SITE_ICONS = (list as { icons: ContentIcon[] }).icons;

type Item = Exclude<Format, "-">;
type Top = { rule: true } | { item: Item } | { group: string; items: Item[] };

/** The list in layers: a group where its first format stood; no line first, last or twice. */
function layers(formats: Format[]): Top[] {
  const out: Top[] = [];
  const groups = new Map<string, Item[]>();
  for (const f of formats) {
    if (f === "-") {
      if (out.length && !("rule" in out[out.length - 1])) out.push({ rule: true });
    } else if (f.group) {
      let g = groups.get(f.group);
      if (!g) {
        g = [];
        groups.set(f.group, g);
        out.push({ group: f.group, items: g });
      }
      g.push(f);
    } else out.push({ item: f });
  }
  while (out.length && "rule" in out[out.length - 1]) out.pop();
  return out;
}

/** A menu strip's rows that fold others into a second strip beside it (here, and 见字如面's 「更多格式」 → 提示框): the
 *  strip comes out when the pointer rests on its row a moment (on its way to the strip it crosses other rows), or on
 *  a click; beside the first strip (the other side when there's no room), level with its row. `moved`: the first
 *  strip's place, measured again when it changes. */
export function useSideStrip(strip: RefObject<HTMLElement | null>, moved: unknown) {
  const side = useRef<HTMLDivElement>(null);
  /** The group whose strip is out, and the row it hangs from. */
  const [sub, setSub] = useState<{ name: string; row: HTMLElement } | null>(null);
  const [at, setAt] = useState<{ left: number; top: number } | null>(null);
  const aim = useRef(0);
  useEffect(() => () => window.clearTimeout(aim.current), []);
  useLayoutEffect(() => {
    const [m, s] = [strip.current, side.current];
    if (!sub || !m || !s || !sub.row.isConnected) return setAt(null);
    const [mr, rr, sr] = [m.getBoundingClientRect(), sub.row.getBoundingClientRect(), s.getBoundingClientRect()];
    const right = mr.right - 3 + sr.width <= window.innerWidth - 8;
    setAt({ left: right ? mr.right - 3 : Math.max(8, mr.left - sr.width + 3), top: Math.max(8, Math.min(rr.top, window.innerHeight - sr.height - 8)) });
  }, [sub, moved]); // eslint-disable-line react-hooks/exhaustive-deps
  const open = (next: { name: string; row: HTMLElement } | null) => {
    window.clearTimeout(aim.current);
    setSub(next);
  };
  return {
    side,
    sub,
    /** The second strip's place (null: not measured yet — keep it hidden). */
    at,
    open,
    /** The pointer came to a row: `next`'s strip (null: none) once it rests there. */
    rest: (next: { name: string; row: HTMLElement } | null) => {
      window.clearTimeout(aim.current);
      if (next?.name === sub?.name) return;
      aim.current = window.setTimeout(() => setSub(next), sub ? 160 : 70);
    },
    /** The pointer reached the second strip: keep it. */
    hold: () => window.clearTimeout(aim.current),
  };
}

/** `libraries`: which icon libraries the list offers (both by default). A list with no formats and one library opens
 *  straight in that library (the website's plain text boxes: 网站图标 only — the website draws nothing else there). */
export function MdMenu(props: { x: number; y: number; formats: Format[]; libraries?: ("pomelo" | "site")[]; host: Element; onFormat: (f: Item) => void; onInsert: (text: string) => void; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const strip = useRef<HTMLDivElement>(null);
  const tops = useMemo(() => layers(props.formats), [props.formats]);
  const libs = props.libraries ?? ["pomelo", "site"];
  const only = props.formats.length === 0 && libs.length === 1 ? libs[0] : null;
  const [view, setView] = useState<"formats" | "pomelo" | "site">(only ?? "formats");
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const { side, sub, at: subPos, open, rest, hold } = useSideStrip(strip, pos);
  useEffect(() => {
    if (view !== "formats") open(null);
  }, [view]); // eslint-disable-line react-hooks/exhaustive-deps
  useLayoutEffect(() => {
    const el = strip.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setPos({ left: Math.max(8, Math.min(props.x, window.innerWidth - width - 8)), top: Math.max(8, Math.min(props.y, window.innerHeight - height - 8)) });
  }, [props.x, props.y, view]);
  useEffect(() => {
    const away = (e: PointerEvent) => !ref.current?.contains(e.target as Node) && props.onClose();
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      if (sub) open(null);
      else if (view === "formats" || only) props.onClose();
      else setView("formats");
    };
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("keydown", key, true);
    window.addEventListener("blur", props.onClose);
    window.addEventListener("resize", props.onClose);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("blur", props.onClose);
      window.removeEventListener("resize", props.onClose);
    };
  });
  const pick = (f: () => void) => {
    props.onClose();
    f();
  };
  const back = (title: string) =>
    only ? (
      <div className="pm-menu-item md-menu-item md-menu-back">{title}</div>
    ) : (
      <button type="button" className="pm-menu-item md-menu-item md-menu-back" onClick={() => setView("formats")}>
        <span className="flex items-center gap-1">
          <ChevronLeft size={14} /> {title}
        </span>
      </button>
    );
  const row = (f: Item, key: number) => (
    <button key={key} type="button" role="menuitem" className={`pm-menu-item md-menu-item ${f.lead ? "md-menu-lead" : ""}`} onMouseEnter={(e) => e.currentTarget.closest(".md-sub") || rest(null)} onClick={() => pick(() => props.onFormat(f))}>
      <span className="flex min-w-0 items-center gap-2">
        {f.icon}
        <span className="truncate">{f.label}</span>
      </span>
      {f.keys && <span className="md-menu-keys">{f.keys}</span>}
    </button>
  );
  const group = sub ? tops.find((t): t is { group: string; items: Item[] } => "group" in t && t.group === sub.name) : undefined;
  return createPortal(
    // the box keeps its focus and selection
    <div ref={ref} onMouseDown={(e) => e.preventDefault()} onContextMenu={(e) => e.preventDefault()}>
    <div
      ref={strip}
      role="menu"
      aria-label="格式和图标"
      style={{ left: pos?.left ?? props.x, top: pos?.top ?? props.y, visibility: pos ? "visible" : "hidden" }}
      className={`pm-menu md-menu ${view === "formats" ? "" : "wide"}`}
    >
      {view === "formats" && (
        <>
          {tops.map((t, i) =>
            "rule" in t ? (
              <div key={i} className="md-menu-rule" />
            ) : "item" in t ? (
              row(t.item, i)
            ) : (
              <button
                key={i}
                type="button"
                role="menuitem"
                aria-haspopup="true"
                aria-expanded={sub?.name === t.group}
                className={`pm-menu-item md-menu-item ${sub?.name === t.group ? "on" : ""}`}
                onMouseEnter={(e) => rest({ name: t.group, row: e.currentTarget })}
                onClick={(e) => open({ name: t.group, row: e.currentTarget })}
              >
                <span>{t.group}</span>
                <ChevronRight size={14} className="md-menu-keys" />
              </button>
            ),
          )}
          {tops.length > 0 && <div className="md-menu-rule" />}
          {libs.includes("pomelo") && (
            <button type="button" role="menuitem" aria-haspopup="true" className="pm-menu-item md-menu-item" onMouseEnter={() => rest(null)} onClick={() => setView("pomelo")}>
              <span>柚儿园图标</span>
              <ChevronRight size={14} className="md-menu-keys" />
            </button>
          )}
          {libs.includes("site") && (
            <button type="button" role="menuitem" aria-haspopup="true" className="pm-menu-item md-menu-item" onMouseEnter={() => rest(null)} onClick={() => setView("site")}>
              <span>网站图标</span>
              <ChevronRight size={14} className="md-menu-keys" />
            </button>
          )}
        </>
      )}
      {view === "pomelo" && (
        <>
          {back("柚儿园图标")}
          <div className="pm-menu-item md-menu-grid">
            <IconGrid cols={9} size={18} onPick={(key) => pick(() => props.onInsert(iconToken(key)))} />
          </div>
        </>
      )}
      {view === "site" && (
        <>
          {back("网站图标")}
          <div className="pm-menu-item md-menu-grid">
            <div className="md-site-grid" role="listbox" aria-label="网站图标">
              {SITE_ICONS.map((i) => (
                <button key={i.name} type="button" role="option" aria-selected={false} title={`:${i.name}:`} onClick={() => pick(() => props.onInsert(`:${i.name}:`))} className="md-site-cell">
                  <span aria-hidden className="md-site-pic" dangerouslySetInnerHTML={{ __html: i.svg }} />
                  <span className="md-site-name">{i.label.zh}</span>
                </button>
              ))}
            </div>
          </div>
        </>
      )}
    </div>
    {view === "formats" && group && (
      <div ref={side} role="menu" aria-label={group.group} className="pm-menu md-menu md-sub" style={{ left: subPos?.left ?? 0, top: subPos?.top ?? 0, visibility: subPos ? "visible" : "hidden" }} onMouseEnter={hold}>
        {group.items.map(row)}
      </div>
    )}
    </div>,
    props.host,
  );
}
