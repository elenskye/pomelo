// The desk (docs/design.md「外框」, the B3 desk of visual-atelier's frame prototype): 首页 lies straight on it; a
// module's page is a sheet of grid paper with stitches down its left edge, tilted a little, its sticker slapped on
// the top-left corner and its pages (subs) as folder tabs (纸签) on the top edge. The other open pages are stacked
// to its right, a paper edge and their sticker showing (hover pulls one out a little, its name hangs under the
// sticker; click brings it to the front). Motion: ds.css (page-stack, slap-in); the sheet is painted (data-hand).
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useLocation, useNavigate } from "react-router";
import { flash, reducedMotion } from "@/shared/hand";
import { HandIcon, Tape, Tip } from "@/shared/handParts";
import { useDock } from "./dock";
import { useModuleMenu } from "./DockPage";
import { Menu } from "./Menu";
import { ModuleIcon } from "./ModuleIcon";
import { moduleById, pageName, splitPath } from "./modules";
import { PomeloFace, type Face } from "./Pomelo";
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

/** 柚子's own page has 柚子 on its corner, and it can be poked (小彩蛋: 5 pokes in 2.5 s and it gets dizzy). */
function PokePomelo(props: { onFace: (f: Face | null, ms: number) => void; onDizzy: (el: HTMLElement) => void }) {
  const [face, setFace] = useState<Face>("idle");
  const [dizzy, setDizzy] = useState(false);
  const pokes = useRef<number[]>([]);
  const timer = useRef(0);
  const set = (f: Face, ms: number) => {
    setFace(f);
    props.onFace(f, ms);
    clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setFace("idle"), ms);
  };
  return (
    <button
      type="button"
      aria-label="戳一下柚子"
      className="slap poke pm-slap-in is-in"
      onClick={(e) => {
        const now = Date.now();
        pokes.current = pokes.current.filter((t) => now - t < 2500).concat(now);
        if (pokes.current.length >= 5 && !dizzy) {
          pokes.current = [];
          setDizzy(!reducedMotion());
          set("error", 2200);
          props.onDizzy(e.currentTarget);
          setTimeout(() => setDizzy(false), 2200);
          return;
        }
        if (!dizzy) set((["wow", "success", "wink", "surprised"] as Face[])[pokes.current.length % 4], 1000);
      }}
    >
      <span style={{ transform: "rotate(-8deg)", display: "inline-flex" }}>
        <PomeloFace face={face} size={72} dizzy={dizzy} />
      </span>
    </button>
  );
}

export function PaperStack(props: { children: ReactNode; onFace: (f: Face | null, ms: number) => void; onDizzy: (el: HTMLElement) => void }) {
  const tabs = useTabs();
  const path = useLocation().pathname;
  const [front, ...behind] = tabs.stack;
  const frontRef = useRef<HTMLDivElement>(null);
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
  if (!front) return null;
  const mod = splitPath(front.path).module;
  const def = moduleById(mod);
  return (
    <div className="stack">
      {behind.slice(0, STACK_SHOWN).map((p, j) => {
        const m = splitPath(p.path).module;
        return (
          <div key={p.id} className="pm-page is-stacked" style={{ "--j": j } as React.CSSProperties}>
            <div className="paper pm-sheet" data-hand="sheet" data-seed={seedOf(m)} data-tilt="-0.3" />
            <button type="button" className="peek pm-page-peek pm-tip-host" aria-label={`换到「${pageName(p.path)}」`} onClick={() => tabs.select(tabs.tabs.findIndex((t) => t.id === p.id))}>
              <ModuleIcon def={moduleById(m)} size={46} />
              <Tip>{pageName(p.path)}</Tip>
            </button>
          </div>
        );
      })}
      <div ref={frontRef} key={front.id} className="pm-page front" aria-label={def.label}>
        <FolderTabs module={mod} path={path.startsWith(`/${mod}`) ? path : front.path} />
        <main className={`paper pm-sheet ${mod === "youzi" ? "lined" : ""}`} data-hand="sheet" data-seed={seedOf(mod)} data-tilt="-0.3">
          <div className="paper-scroll">{props.children}</div>
        </main>
        {mod === "youzi" ? (
          <PokePomelo onFace={props.onFace} onDizzy={props.onDizzy} />
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
