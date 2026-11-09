// 开着的页和前进后退 (requirements「界面外框」, the B3 desk decided 2026-11-08): 首页 is the desk, not a page; every
// module you open is a page lying on it — the one in front is open, the others are stacked to its right (their
// paper edge and sticker showing) and line up in the top bar's「桌上开着」. One page per module (its path says
// which of its pages: `/site/works`). ‹ › walk the pages you went through, in order (going to the desk, opening
// the 柚子瓣 / 贴纸簿 / 全局搜索, closing a page aren't steps); a step back to a page closed since goes to the desk.
// - Navigation surfaces (贴纸簿, 柚子瓣, 全局搜索, the top bar) use `open` (or `goHome`); links inside pages just
//   navigate and this records them. 「＋」 (⌘T) opens the 贴纸簿 (`book`) to pick a page.
// - ‹ › buttons, mouse side buttons, ⌘[ ⌘], ⌘← ⌘→ (not while typing).
// - The open pages are kept in settings `ui.tabs` (their paths); on launch they come back stacked, the desk in front.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useLocation, useNavigate, useNavigationType } from "react-router";
import { api } from "@/shared/ipc";
import { moduleById, splitPath } from "./modules";

const SAVED = "ui.tabs";
const OPEN_ONCE = "dev.open";
const MAX_STEPS = 50;
/** Pages stacked behind the front one (more stay open but don't show). */
export const STACK_SHOWN = 4;

type Page = { id: number; path: string; seen: number };
type State = { pages: Page[]; front: number | null; hist: string[]; i: number; book: boolean };

export type TabsApi = {
  /** The open pages in the order they were opened (the top bar's「桌上开着」). */
  tabs: { id: number; path: string }[];
  /** The front page first, then the others by when they were last in front (the stack on the desk). `seen` grows
   *  each time a page comes to the front: a page behind keeps its own, so the pile's order can be told without
   *  renumbering the pages (Desk.tsx). */
  stack: { id: number; path: string; seen: number }[];
  /** Index in `tabs` of the page in front; -1 on the desk. */
  active: number;
  home: boolean;
  canBack: boolean;
  canForward: boolean;
  back: () => void;
  forward: () => void;
  /** The steps behind (nearest first) or ahead. */
  trail: (dir: -1 | 1) => { index: number; path: string }[];
  jump: (index: number) => void;
  /** Brings an open page (by index in `tabs`) to the front. */
  select: (i: number) => void;
  close: (i: number) => void;
  /** 「＋」: the 贴纸簿, to pick a page. */
  newTab: () => void;
  goHome: () => void;
  /** Opens a page from a navigation surface (bringing its module's page to the front when it's open). */
  open: (path: string, state?: unknown) => void;
  /** The page settled which of its pages it shows (a bare `/site` is really `/site/works`). */
  settle: (path: string) => void;
  /** The 贴纸簿 is open. */
  book: boolean;
  setBook: (open: boolean) => void;
};

const Ctx = createContext<TabsApi | null>(null);

export function useTabs(): TabsApi {
  const t = useContext(Ctx);
  if (!t) throw new Error("useTabs outside TabsProvider");
  return t;
}

const editing = (el: Element | null) => !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || (el as HTMLElement).isContentEditable);
const moduleOf = (path: string) => splitPath(path).module;
/** The desk; the old 模块坞 address (`/dock`) is the 贴纸簿 over the desk now. */
const isHome = (path: string) => moduleOf(path) === "" || moduleOf(path) === "dock";
const where = (s: State) => (s.front == null ? "/" : (s.pages.find((p) => p.id === s.front)?.path ?? "/"));
const stepped = (s: State, path: string): State => {
  if (s.hist[s.i] === path) return s;
  const hist = [...s.hist.slice(0, s.i + 1), path].slice(-MAX_STEPS);
  return { ...s, hist, i: hist.length - 1 };
};

export function TabsProvider(props: { children: ReactNode }) {
  const navigate = useNavigate();
  const location = useLocation();
  const navType = useNavigationType();
  const path = location.pathname;
  const seq = useRef(1);
  const clock = useRef(1);
  const [st, setSt] = useState<State>(() => {
    const s: State = { pages: [], front: null, hist: ["/"], i: 0, book: moduleOf(path) === "dock" };
    if (isHome(path)) return s;
    const page = { id: seq.current++, path, seen: clock.current++ };
    return { ...s, hist: [path], pages: [page], front: page.id };
  });
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

  /** `path` in front (its module's page, opened when it isn't): the state, not yet a step. */
  const toFront = (s: State, to: string): State => {
    if (isHome(to)) return { ...s, front: null };
    const mod = moduleOf(to);
    const had = s.pages.find((p) => moduleOf(p.path) === mod);
    if (had) return { ...s, front: had.id, pages: s.pages.map((p) => (p.id === had.id ? { ...p, path: to, seen: clock.current++ } : p)) };
    const page = { id: seq.current++, path: to, seen: clock.current++ };
    return { ...s, front: page.id, pages: [...s.pages, page] };
  };

  // launch: the pages from last time, stacked, the desk in front — unless the setting `dev.open` names a page: that
  // one is in front, once (testing a page in the real window without clicking there: docs/testing.md「真实窗口」)
  useEffect(() => {
    Promise.all([api.settingsGet<unknown>(SAVED), api.settingsGet<unknown>(OPEN_ONCE).catch(() => null)])
      .then(([v, once]) => {
        const paths = Array.isArray(v) ? v.filter((p): p is string => typeof p === "string" && p.startsWith("/") && !isHome(p)).slice(0, 20) : [];
        const first = typeof once === "string" && once.startsWith("/") && !isHome(once) ? once : null;
        if (first) void api.settingsSet(OPEN_ONCE, null).catch(() => {});
        if (!paths.length && !first) return;
        const seen = new Set<string>();
        const pages = paths.filter((p) => !seen.has(moduleOf(p)) && (seen.add(moduleOf(p)), true)).map((p) => ({ id: seq.current++, path: p, seen: clock.current++ }));
        const base = { pages, front: null, hist: ["/"], i: 0, book: false };
        setSt(first ? { ...toFront(base, first), hist: ["/", first], i: 1 } : base);
        show(first ?? "/");
      })
      .catch(() => {})
      .finally(() => setRestored(true));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!restored) return;
    const t = setTimeout(() => api.settingsSet(SAVED, st.pages.map((p) => p.path)).catch(() => {}), 400);
    return () => clearTimeout(t);
  }, [st.pages, restored]);

  // record navigation that didn't come from here (links inside a page)
  useEffect(() => {
    if (expect.current === path) {
      expect.current = null;
      return;
    }
    expect.current = null;
    if (moduleOf(path) === "dock") {
      setSt((s) => ({ ...s, book: true }));
      show(where(stRef.current));
      return;
    }
    setSt((s) => {
      const cur = where(s);
      if (cur === path) return s;
      if (navType !== "PUSH") {
        // a page clearing its state on a bare path keeps the page it settled on
        const a = splitPath(path), b = splitPath(cur);
        if (!a.sub && a.module === b.module) return s;
        const next = toFront(s, path);
        const hist = [...next.hist];
        hist[next.i] = path;
        return { ...next, hist };
      }
      return stepped(toFront(s, path), path);
    });
    // the path too: an address changed from outside the app comes with the same key ("default") every time
  }, [location.key, path]); // eslint-disable-line react-hooks/exhaustive-deps

  /** Goes to a step of the trail: its page in front when it's still open, else the desk. */
  const jumpTo = useCallback(
    (index: number) => {
      const s = stRef.current;
      if (index < 0 || index >= s.hist.length || index === s.i) return;
      const to = s.hist[index];
      const open = isHome(to) || s.pages.some((p) => moduleOf(p.path) === moduleOf(to));
      const next = { ...(open ? toFront(s, to) : { ...s, front: null }), i: index, book: false };
      setSt(next);
      show(where(next));
    },
    [show], // eslint-disable-line react-hooks/exhaustive-deps
  );
  const step = useCallback((d: -1 | 1) => jumpTo(stRef.current.i + d), [jumpTo]);

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
    const tabs = st.pages.map((p) => ({ id: p.id, path: p.path }));
    const front = st.pages.find((p) => p.id === st.front);
    const behind = st.pages.filter((p) => p.id !== st.front).sort((a, b) => b.seen - a.seen);
    const goTo = (to: string, state?: unknown) => {
      const s = stRef.current;
      const next = stepped({ ...toFront(s, to), book: false }, isHome(to) ? "/" : to);
      setSt(next);
      show(isHome(to) ? "/" : to, state);
    };
    return {
      tabs,
      stack: [...(front ? [front] : []), ...behind].map((p) => ({ id: p.id, path: p.path, seen: p.seen })),
      active: front ? tabs.findIndex((t) => t.id === front.id) : -1,
      home: !front,
      canBack: st.i > 0,
      canForward: st.i < st.hist.length - 1,
      back: () => step(-1),
      forward: () => step(1),
      trail: (dir) => {
        const idx = st.hist.map((_, k) => k);
        const ks = dir < 0 ? idx.slice(0, st.i).reverse() : idx.slice(st.i + 1);
        return ks.map((k) => ({ index: k, path: st.hist[k] }));
      },
      jump: jumpTo,
      select: (i) => {
        const p = stRef.current.pages[i];
        if (p && p.id !== stRef.current.front) goTo(p.path);
      },
      close: (i) => {
        const s = stRef.current;
        const gone = s.pages[i];
        if (!gone) return;
        const pages = s.pages.filter((p) => p.id !== gone.id);
        let front = s.front;
        if (front === gone.id) front = [...pages].sort((a, b) => b.seen - a.seen)[0]?.id ?? null;
        const next = { ...s, pages, front };
        setSt(next);
        if (s.front === gone.id) show(where(next));
      },
      newTab: () => setSt((s) => ({ ...s, book: true })),
      goHome: () => goTo("/"),
      open: (to, state) => {
        if (moduleOf(to) === "dock") return setSt((s) => ({ ...s, book: true }));
        const target0 = moduleById(moduleOf(to)).to ?? to;
        // the module's own last page when the path names none
        const had = stRef.current.pages.find((p) => moduleOf(p.path) === moduleOf(target0));
        const target = had && !splitPath(target0).sub ? had.path : target0;
        goTo(target, state);
      },
      settle: (to) => {
        setSt((s) => {
          const cur = where(s);
          if (cur === to || moduleOf(cur) !== moduleOf(to) || splitPath(cur).sub) return s;
          const hist = [...s.hist];
          if (hist[s.i] === cur) hist[s.i] = to;
          return { ...s, hist, pages: s.pages.map((p) => (p.id === s.front ? { ...p, path: to } : p)) };
        });
      },
      book: st.book,
      setBook: (open) => setSt((s) => (s.book === open ? s : { ...s, book: open })),
    };
  }, [st, step, jumpTo, show]); // eslint-disable-line react-hooks/exhaustive-deps

  return <Ctx.Provider value={api_}>{props.children}</Ctx.Provider>;
}
