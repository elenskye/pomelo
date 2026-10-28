// 页签 and 前进后退 (requirements「界面外框」): like Safari, each 页签 keeps its own trail of pages — entering a
// module or one of its pages (`/site/works`) is a step; opening a record isn't. The router always shows the
// active 页签's page; switching 页签 opens that page again (the page starts fresh).
// - Navigation surfaces (索引签, 模块坞, 柚子瓣, 全局搜索) use `open`: a module already showing in another
//   页签 switches there. Links inside pages just navigate; this records them.
// - ‹ › buttons, mouse side buttons, ⌘[ ⌘], ⌘← ⌘→ (not while typing).
// - The 页签 (each one's current page, not its trail) are kept in settings `ui.tabs`; on launch they come
//   back with 首页 in front (a 首页 页签 is added when none was open).
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useLocation, useNavigate, useNavigationType } from "react-router";
import { api } from "@/shared/ipc";
import { splitPath } from "./modules";

const SAVED = "ui.tabs";
const MAX_STEPS = 50;

type Tab = { id: number; hist: string[]; i: number };
type State = { tabs: Tab[]; active: number };

export type TabsApi = {
  tabs: { id: number; path: string }[];
  active: number;
  canBack: boolean;
  canForward: boolean;
  back: () => void;
  forward: () => void;
  /** The steps behind (nearest first) or ahead. */
  trail: (dir: -1 | 1) => { index: number; path: string }[];
  jump: (index: number) => void;
  select: (i: number) => void;
  close: (i: number) => void;
  newTab: () => void;
  /** Opens a page from a navigation surface (switching to a 页签 that already shows its module). */
  open: (path: string, state?: unknown) => void;
  /** The page settled which of its pages it shows (a bare `/site` is really `/site/works`). */
  settle: (path: string) => void;
};

const Ctx = createContext<TabsApi | null>(null);

export function useTabs(): TabsApi {
  const t = useContext(Ctx);
  if (!t) throw new Error("useTabs outside TabsProvider");
  return t;
}

const current = (t: Tab) => t.hist[t.i];
const withTab = (s: State, i: number, t: Tab): State => ({ ...s, tabs: s.tabs.map((x, k) => (k === i ? t : x)) });
const pushed = (t: Tab, path: string): Tab => {
  if (current(t) === path) return t;
  const hist = [...t.hist.slice(0, t.i + 1), path].slice(-MAX_STEPS);
  return { ...t, hist, i: hist.length - 1 };
};

const editing = (el: Element | null) => !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || (el as HTMLElement).isContentEditable);

export function TabsProvider(props: { children: ReactNode }) {
  const navigate = useNavigate();
  const location = useLocation();
  const navType = useNavigationType();
  const path = location.pathname;
  const seq = useRef(2);
  const [st, setSt] = useState<State>(() => ({ tabs: [{ id: 1, hist: [path], i: 0 }], active: 0 }));
  const stRef = useRef(st);
  stRef.current = st;
  /** The path our own navigation is about to show (not a step to record). */
  const expect = useRef<string | null>(null);
  const [restored, setRestored] = useState(false);

  const show = useCallback(
    (to: string, state?: unknown) => {
      expect.current = to;
      void navigate(to, { replace: true, state });
    },
    [navigate],
  );

  // launch: the 页签 from last time, 首页 in front
  useEffect(() => {
    api
      .settingsGet<unknown>(SAVED)
      .then((v) => {
        const paths = Array.isArray(v) ? v.filter((p): p is string => typeof p === "string" && p.startsWith("/")).slice(0, 20) : [];
        if (!paths.length) return;
        let tabs = paths.map((p) => ({ id: seq.current++, hist: [p], i: 0 }));
        let active = paths.indexOf("/");
        if (active < 0) {
          tabs = [{ id: seq.current++, hist: ["/"], i: 0 }, ...tabs];
          active = 0;
        }
        setSt({ tabs, active });
        show("/");
      })
      .catch(() => {})
      .finally(() => setRestored(true));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!restored) return;
    const t = setTimeout(() => api.settingsSet(SAVED, st.tabs.map(current)).catch(() => {}), 400);
    return () => clearTimeout(t);
  }, [st, restored]);

  // record navigation that didn't come from here
  useEffect(() => {
    if (expect.current === path) {
      expect.current = null;
      return;
    }
    expect.current = null;
    setSt((s) => {
      const t = s.tabs[s.active];
      const cur = current(t);
      if (cur === path) return s;
      if (navType !== "PUSH") {
        // a page clearing its state on a bare path keeps the page it settled on
        const a = splitPath(path), b = splitPath(cur);
        if (!a.sub && a.module === b.module) return s;
        const hist = [...t.hist];
        hist[t.i] = path;
        return withTab(s, s.active, { ...t, hist });
      }
      return withTab(s, s.active, pushed(t, path));
    });
  }, [location.key]); // eslint-disable-line react-hooks/exhaustive-deps

  const jumpTo = useCallback(
    (index: number) => {
      const s = stRef.current;
      const t = s.tabs[s.active];
      if (index < 0 || index >= t.hist.length || index === t.i) return;
      setSt(withTab(s, s.active, { ...t, i: index }));
      show(t.hist[index]);
    },
    [show],
  );
  const step = useCallback((d: -1 | 1) => jumpTo(stRef.current.tabs[stRef.current.active].i + d), [jumpTo]);

  // ⌘[ ⌘] anywhere, ⌘← ⌘→ when not typing; mouse side buttons (3 back, 4 forward)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || e.shiftKey || e.altKey || e.ctrlKey) return;
      const arrow = e.key === "ArrowLeft" || e.key === "ArrowRight";
      if (!arrow && e.key !== "[" && e.key !== "]") return;
      if (document.querySelector("dialog[open]")) return;
      if (arrow && editing(document.activeElement)) return;
      e.preventDefault();
      step(e.key === "[" || e.key === "ArrowLeft" ? -1 : 1);
    };
    const side = (e: MouseEvent) => e.button === 3 || e.button === 4;
    const stop = (e: MouseEvent) => side(e) && e.preventDefault();
    const onUp = (e: MouseEvent) => {
      if (!side(e)) return;
      e.preventDefault();
      if (!document.querySelector("dialog[open]")) step(e.button === 3 ? -1 : 1);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", stop);
    window.addEventListener("auxclick", stop);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", stop);
      window.removeEventListener("auxclick", stop);
      window.removeEventListener("mouseup", onUp);
    };
  }, [step]);

  const api_: TabsApi = useMemo(() => {
    const t = st.tabs[st.active];
    return {
      tabs: st.tabs.map((x) => ({ id: x.id, path: current(x) })),
      active: st.active,
      canBack: t.i > 0,
      canForward: t.i < t.hist.length - 1,
      back: () => step(-1),
      forward: () => step(1),
      trail: (dir) => {
        const idx = t.hist.map((_, k) => k);
        const ks = dir < 0 ? idx.slice(0, t.i).reverse() : idx.slice(t.i + 1);
        return ks.map((k) => ({ index: k, path: t.hist[k] }));
      },
      jump: jumpTo,
      select: (i) => {
        const s = stRef.current;
        if (i === s.active || !s.tabs[i]) return;
        setSt({ ...s, active: i });
        show(current(s.tabs[i]));
      },
      close: (i) => {
        const s = stRef.current;
        let tabs = s.tabs.filter((_, k) => k !== i);
        if (!tabs.length) tabs = [{ id: seq.current++, hist: ["/"], i: 0 }];
        const active = i < s.active ? s.active - 1 : Math.min(s.active, tabs.length - 1);
        setSt({ tabs, active });
        if (i === s.active) show(current(tabs[active]));
      },
      newTab: () => {
        const s = stRef.current;
        const tabs = [...s.tabs, { id: seq.current++, hist: ["/dock"], i: 0 }];
        setSt({ tabs, active: tabs.length - 1 });
        show("/dock");
      },
      open: (to, state) => {
        const s = stRef.current;
        const mod = splitPath(to).module;
        const other = s.tabs.findIndex((x, k) => k !== s.active && splitPath(current(x)).module === mod);
        const i = other >= 0 ? other : s.active;
        const t = s.tabs[i];
        // the module's own last page when the path names none
        const target = !splitPath(to).sub && splitPath(current(t)).module === mod ? current(t) : to;
        setSt(withTab({ ...s, active: i }, i, pushed(t, target)));
        show(target, state);
      },
      settle: (to) => {
        setSt((s) => {
          const x = s.tabs[s.active];
          const cur = current(x);
          if (cur === to || splitPath(cur).module !== splitPath(to).module || splitPath(cur).sub) return s;
          const hist = [...x.hist];
          hist[x.i] = to;
          return withTab(s, s.active, { ...x, hist });
        });
      },
    };
  }, [st, step, jumpTo, show]);

  return <Ctx.Provider value={api_}>{props.children}</Ctx.Provider>;
}
