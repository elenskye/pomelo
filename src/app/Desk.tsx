// The desk (docs/design.md「外框」, the B3 desk of visual-atelier's frame prototype): 首页 lies straight on it; a
// module's page is a sheet of grid paper with stitches down its left edge, tilted a little, its sticker slapped on
// the top-left corner and its pages (subs) as folder tabs (纸签) on the top edge. The other open pages are stacked
// to its right, a paper edge and their sticker showing (hover pulls one out a little, its name hangs under the
// sticker; a click anywhere on what shows of it brings it to the front). Motion: ds.css (page-stack, slap-in); the
// sheet is painted (data-hand).
import { createContext, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useLocation, useNavigate } from "react-router";
import { flash, reducedMotion } from "@/shared/hand";
import { HandIcon, Tape, Tip } from "@/shared/handParts";
import { useDock } from "./dock";
import { useModuleMenu } from "./DockPage";
import { Menu } from "./Menu";
import { ModuleIcon } from "./ModuleIcon";
import { moduleById, pageName, splitPath } from "./modules";
import { rememberSub, subNav } from "./subpage";
import { STACK_SHOWN, useTabs } from "./tabs";

/** A seed per module, so its paper's wobble stays the same. */
const seedOf = (id: string) => 900 + ([...id].reduce((a, c) => a + c.charCodeAt(0), 0) % 97) * 3;
const TAB_FILLS = ["cat-mint-bg", "cat-sky-bg", "cat-orange-bg", "cat-grape-bg", "cat-pink-bg"];

/** The module's pages as folder tabs on the paper's top edge. */
function FolderTabs(props: { module: string; path: string }) {
  const navigate = useNavigate();
  const subs = moduleById(props.module).subs;
  if (!subs || subs.length < 2) return null;
  const cur = splitPath(props.path).sub ?? subs[0].value;
  return (
    <div className="ftabs" role="tablist" aria-label={`${moduleById(props.module).label}的子页`}>
      {subs.map((s, i) => {
        const on = s.value === cur;
        return (
          <button
            key={s.value}
            type="button"
            role="tab"
            aria-selected={on}
            onClick={() => {
              if (on) return;
              const own = subNav(props.module);
              if (own) return own(s.value);
              rememberSub(props.module, s.value);
              void navigate(`/${props.module}/${s.value}`);
            }}
            className="ft pm-folder-tab"
          >
            <span className="ft-off" data-hand="frame" data-r="9" data-sw="1.5" data-amp="0.5" data-fill={TAB_FILLS[i % 5]} data-seed={800 + i} />
            <span className="ft-on" data-hand="frame" data-r="9" data-sw="1.7" data-amp="0.5" data-fill="paper" data-seed={810 + i} />
            <span className="ft-t">{s.label}</span>
          </button>
        );
      })}
    </div>
  );
}

/** A place on the desk beside the front page, for the page to fill (柚子's page: the big pomelo and the slips of
 *  what was talked about, Phase 2 final design). Null on other pages. */
const SideCtx = createContext<HTMLElement | null>(null);
export const useDeskSide = () => useContext(SideCtx);

const RINGS = Array.from({ length: 64 }, (_, i) => i);

/** A value that follows `value` after `ms`, starting from `first` (the pile behind the front page: the front page
 *  paints first, also when coming back from the desk). */
function useLate<T>(value: T, ms: number, first: T): T {
  const [late, setLate] = useState(first);
  useEffect(() => {
    const t = setTimeout(() => setLate(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return late;
}

/** The pages on the desk. Smoothness (docs/testing.md「流畅度」): a page behind keeps its own element and layer while
 *  it lies there — rendered in a fixed order (by id), its z-index its own `seen` (never renumbered: WebKit repaints a
 *  whole layer whose z-index changes), only its place in the pile changes (its transform, written on the element: a custom property
 *  would be inherited by everything in the page and restyle it), which the compositor moves without repainting — and the pile is brought up to date a moment after the front page (it paints first). */
export function PaperStack(props: { children: ReactNode }) {
  const tabs = useTabs();
  const path = useLocation().pathname;
  const [front, ...behind] = tabs.stack;
  const frontRef = useRef<HTMLDivElement>(null);
  const [side, setSide] = useState<HTMLElement | null>(null);
  const was = useRef<{ id: number | null; ids: number[] }>({ id: null, ids: [] });
  // a page that wasn't on the desk slides in from the right; any page coming to the front slaps its sticker again
  // (before the first paint, so it doesn't show in place for a frame first)
  useLayoutEffect(() => {
    if (!front) return;
    const prev = was.current;
    if (front.id !== prev.id && !reducedMotion()) {
      if (!prev.ids.includes(front.id)) flash(frontRef.current, "is-in", 520);
      flash(frontRef.current?.querySelector(".slap"), "is-in");
    }
    was.current = { id: front.id, ids: tabs.stack.map((p) => p.id) };
  }, [front?.id, tabs.stack]); // eslint-disable-line react-hooks/exhaustive-deps
  const pile = useLate(behind.slice(0, STACK_SHOWN).map((p) => p.id).join(), 240, "");
  if (!front) return null;
  const mod = splitPath(front.path).module;
  const def = moduleById(mod);
  const youzi = mod === "youzi";
  const places = new Map(pile ? pile.split(",").map((id, j) => [Number(id), j]) : []);
  const stacked = behind.filter((p) => places.has(p.id)).sort((a, b) => a.id - b.id);
  return (
    <div className="stack">
      {stacked.map((p) => {
        const m = splitPath(p.path).module;
        const j = places.get(p.id) ?? 0;
        return (
          // the whole page that shows is the way to it, not only its sticker
          <div key={p.id} className="pm-page is-stacked" style={{ transform: `translate(${22 * (j + 1)}px, ${4 * j}px) rotate(${0.6 + 0.8 * j}deg)`, zIndex: p.seen }} onClick={() => tabs.select(tabs.tabs.findIndex((t) => t.id === p.id))}>
            <div className="paper pm-sheet" data-hand="sheet" data-seed={seedOf(m)} data-tilt="-0.3" />
            <button type="button" className="peek pm-page-peek pm-tip-host" style={{ top: 80 + 92 * j }} aria-label={`换到「${pageName(p.path)}」`}>
              <ModuleIcon def={moduleById(m)} size={46} />
              <Tip>{pageName(p.path)}</Tip>
            </button>
          </div>
        );
      })}
      {youzi && <div ref={setSide} className="deskside" />}
      <div ref={frontRef} key={front.id} className="pm-page front" aria-label={def.label}>
        <FolderTabs module={mod} path={path.startsWith(`/${mod}`) ? path : front.path} />
        <main className={`paper pm-sheet ${youzi ? "lined" : ""}`} data-hand="sheet" data-seed={seedOf(mod)} data-tilt="-0.3">
          <div className="paper-scroll">
            <SideCtx.Provider value={youzi ? side : null}>{props.children}</SideCtx.Provider>
          </div>
        </main>
        {youzi ? (
          // 柚子's page is a spiral notepad: the rings along its top edge (柚子 itself sits on the desk beside it)
          <span className="rings" aria-hidden>
            {RINGS.map((i) => (
              <i key={i} />
            ))}
          </span>
        ) : (
          <span className="slap pm-slap-in is-in" style={{ "--pm-delay": ".3s" } as React.CSSProperties} aria-hidden>
            <span style={{ transform: "rotate(-8deg)", display: "inline-flex" }}>
              <ModuleIcon def={def} size={64} />
            </span>
          </span>
        )}
      </div>
    </div>
  );
}

const TILTS = [-6, 4, -3, 6, -5, 3, -4, 5];

/** The desk's bottom row「常用」: the pinned modules' stickers (click opens, right-click is its menu), then the 贴纸簿. */
export function DeskCommon() {
  const tabs = useTabs();
  const dock = useDock();
  const menu = useModuleMenu(dock);
  return (
    <nav className="common" aria-label="常用">
      <span className="clabel">
        <Tape kind="mint" rot={-6} />
        <span className="tl">常用</span>
      </span>
      {dock.pinned.map((id, i) => {
        const def = dock.defs.get(id) ?? moduleById(id);
        return (
          <button key={id} type="button" className="dstk pm-press" onClick={() => tabs.open(`/${id}`)} onContextMenu={(e) => menu.at(e, id)}>
            <span className="dic pm-lift">
              <span style={{ transform: `rotate(${TILTS[i % TILTS.length]}deg)`, display: "inline-flex" }}>
                <ModuleIcon def={def} size={56} />
              </span>
            </span>
            <span className="dlab">{def.label}</span>
          </button>
        );
      })}
      <button type="button" className="dstk pm-press" onClick={() => tabs.setBook(true)}>
        <span className="dic">
          <span className="bookic pm-lift" data-hand="round" data-seed="750" data-fill="card">
            <HandIcon name="grid" size={24} seed={17} />
          </span>
        </span>
        <span className="dlab">贴纸簿</span>
      </button>
      {menu.open && <Menu {...menu.open} onClose={menu.close} />}
      {menu.dialog}
    </nav>
  );
}
