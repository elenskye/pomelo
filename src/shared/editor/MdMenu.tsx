// The list a right-click opens in every box where I write Markdown (LiveText: 念念不忘, 速记小窗, 呦呦漫游, the
// prompts): the formats that box's flavor knows (加粗, 列表, 标题…), then the two icon libraries —
// 柚儿园图标 (Pomelo's own: goes in as `:pm-<key>:` and is drawn in place, here and wherever Pomelo shows the text)
// and 网站图标 (the website's content icons, synced as src/vendor/pomelo/content-icons.json: goes in as `:name:`,
// which only the website draws — written anywhere else in Pomelo it stays those letters).
// The Design System's menu (a strip of paper, pm-menu); a library opens in the same strip. Esc, a click outside or
// a choice closes it; the box keeps its focus and selection.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
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

export function MdMenu(props: { x: number; y: number; formats: Format[]; host: Element; onFormat: (f: Item) => void; onInsert: (text: string) => void; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [view, setView] = useState<"formats" | "pomelo" | "site">("formats");
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
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
      if (view === "formats") props.onClose();
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
  const back = (title: string) => (
    <button type="button" className="pm-menu-item md-menu-item md-menu-back" onClick={() => setView("formats")}>
      <span className="flex items-center gap-1">
        <ChevronLeft size={14} /> {title}
      </span>
    </button>
  );
  return createPortal(
    <div
      ref={ref}
      role="menu"
      aria-label="格式和图标"
      onMouseDown={(e) => e.preventDefault()} // the box keeps its focus and selection
      onContextMenu={(e) => e.preventDefault()}
      style={{ left: pos?.left ?? props.x, top: pos?.top ?? props.y, visibility: pos ? "visible" : "hidden" }}
      className={`pm-menu md-menu ${view === "formats" ? "" : "wide"}`}
    >
      {view === "formats" && (
        <>
          {props.formats.map((f, i) =>
            f === "-" ? (
              <div key={i} className="md-menu-rule" />
            ) : (
              <button key={i} type="button" role="menuitem" className="pm-menu-item md-menu-item" onClick={() => pick(() => props.onFormat(f))}>
                <span>{f.label}</span>
                {f.keys && <span className="md-menu-keys">{f.keys}</span>}
              </button>
            ),
          )}
          {props.formats.length > 0 && <div className="md-menu-rule" />}
          <button type="button" role="menuitem" aria-haspopup="true" className="pm-menu-item md-menu-item" onClick={() => setView("pomelo")}>
            <span>柚儿园图标</span>
            <ChevronRight size={14} className="md-menu-keys" />
          </button>
          <button type="button" role="menuitem" aria-haspopup="true" className="pm-menu-item md-menu-item" onClick={() => setView("site")}>
            <span>网站图标</span>
            <ChevronRight size={14} className="md-menu-keys" />
          </button>
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
    </div>,
    props.host,
  );
}
