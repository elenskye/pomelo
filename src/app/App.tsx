import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Outlet, useLocation } from "react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderOpen, TriangleAlert } from "@/shared/icons";
import { errorText, useToast } from "@/shared/feedback";
import { api, type AppInfo } from "@/shared/ipc";
import { Button } from "@/shared/ui";
import { IDLE_MS, VAULT_KEY, VAULT_STATUS_KEY, vaultApi, type VaultStatus } from "@/modules/vault/api";
import { LIBRARY_KEY, useYouziStatus } from "@/modules/youzi/api";
import { Bloom } from "./Bloom";
import { DeskCommon, PaperStack } from "./Desk";
import { StickerBook } from "./StickerBook";
import { CheerLayer } from "@/shared/cheer";
import type { Face } from "./Pomelo";
import { useDock } from "./dock";
import { DragChip } from "./drag";
import { useHiddenModules } from "./hidden";
import { useLookColors } from "./moduleLooks";
import { moduleLabel } from "./modules";
import { usePublishedContext, type PageContext } from "./pageContext";
import { reportProblem, useJobWatch } from "./Pomelo";
import { SearchBox } from "./Search";
import { TabsProvider, useTabs } from "./tabs";
import { TopBar } from "./TopBar";
import "./frame.css";

// 柚子's 小窗 brings the Markdown / formula libraries: loaded the first time it opens
const SideChat = lazy(() => import("@/modules/youzi/SideChat").then((m) => ({ default: m.SideChat })));
/** Below this the 小窗 floats over the page instead of making room. */
const FLOAT_QUERY = "(max-width: 1239px)";

function useMedia(query: string) {
  const [on, setOn] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const m = window.matchMedia(query);
    const change = () => setOn(m.matches);
    m.addEventListener("change", change);
    return () => m.removeEventListener("change", change);
  }, [query]);
  return on;
}

/** ⌘J opens / closes the 小窗 (not over a dialog). */
function useChatKey(enabled: boolean, toggle: () => void) {
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || e.key.toLowerCase() !== "j" || e.shiftKey || e.altKey || e.ctrlKey) return;
      if (document.querySelector("dialog[open]")) return;
      e.preventDefault();
      toggle();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enabled, toggle]);
}

/** The menu bar's 页签 menu (Rust menu.rs: ⌘T ⌘W ⌃Tab ⌃⇧Tab ⌘1–⌘9, 设置… ⌘,) and ⌘⇧[ ⌘⇧] (Safari's other keys
 * for the previous / next page). ⌘T opens the 贴纸簿; ⌘1–⌘9: the desk, the 贴纸簿, then the pinned modules. */
function useTabKeys(enabled: boolean) {
  const tabs = useTabs();
  const dock = useDock();
  const now = useRef({ tabs, pinned: dock.pinned });
  now.current = { tabs, pinned: dock.pinned };
  const names = ["桌面", "贴纸簿", ...dock.pinned.map(moduleLabel)].slice(0, 9).join("\n");
  useEffect(() => {
    if (enabled) api.appMenuIndex(names.split("\n")).catch(() => {});
  }, [enabled, names]);
  useEffect(() => {
    if (!enabled) return;
    const act = (action: string) => {
      if (document.querySelector("dialog[open]")) return;
      const { tabs: t, pinned } = now.current;
      const n = t.tabs.length;
      if (action === "new-tab") t.newTab();
      else if (action === "close-tab") {
        if (t.active >= 0) t.close(t.active);
      } else if (action === "next-tab" || action === "prev-tab") {
        if (!n) return;
        const d = action === "next-tab" ? 1 : n - 1;
        t.select(t.active < 0 ? 0 : (t.active + d) % n);
      } else if (action === "settings") t.open("/settings");
      else if (action.startsWith("go:")) {
        const i = Number(action.slice(3));
        if (i === 0) t.goHome();
        else if (i === 1) t.newTab();
        else if (pinned[i - 2]) t.open(`/${pinned[i - 2]}`);
      }
    };
    const off = listen<string>("app-menu", (e) => act(e.payload));
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || !e.shiftKey || e.altKey || e.ctrlKey || (e.code !== "BracketLeft" && e.code !== "BracketRight")) return;
      e.preventDefault();
      act(e.code === "BracketLeft" ? "prev-tab" : "next-tab");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      void off.then((f) => f());
      window.removeEventListener("keydown", onKey);
    };
  }, [enabled]);
}

/** The 速记小窗 (Rust capture.rs): what it saved shows here at once; a conversation kept there and a record a
 *  reply links open here (in 柚子 / the module, as from 全局搜索). */
function useCapture(enabled: boolean) {
  const qc = useQueryClient();
  const tabs = useTabs();
  const open = useRef(tabs.open);
  open.current = tabs.open;
  useEffect(() => {
    if (!enabled) return;
    const off = [
      listen("capture-saved", () => void qc.invalidateQueries()),
      listen<number>("capture-chat", (e) =>
        void qc.invalidateQueries({ queryKey: LIBRARY_KEY }).then(() =>
          open.current("/youzi", { open: { module: "youzi", kind: "session", id: e.payload, title: "", detail: null, parent: null, day: null, amount: null, currency: null, flow: null } }),
        ),
      ),
      listen<{ path: string; hit: unknown }>("capture-open", (e) => open.current(e.payload.path, { open: e.payload.hit })),
    ];
    return () => off.forEach((p) => void p.then((f) => f()));
  }, [enabled, qc]);
}

/** Where the page is when it says nothing itself: its route (芝麻开门: nothing at all). */
function routeContext(path: string): PageContext | null {
  const id = path.split("/")[1] ?? "";
  if (id === "vault") return null;
  return { module: id === "" ? "home" : id };
}

/** Daily chores: Rust takes the day's first snapshot at launch (this covers an app left open past
 * midnight), and the sweep completes finished check-ins and posts due 固定支出 (launch + new day). */
function useDailyChores(enabled: boolean) {
  const qc = useQueryClient();
  useEffect(() => {
    if (!enabled) return;
    const sweep = () =>
      api
        .dailySweep()
        .then((r) => {
          if (r.closed || r.posted) void qc.invalidateQueries();
        })
        .catch(() => {});
    void sweep();
    let day = new Date().toDateString();
    const check = () => {
      const today = new Date().toDateString();
      if (today === day) return;
      day = today;
      void sweep();
      api
        .backupDaily()
        .then((b) => b && qc.invalidateQueries({ queryKey: ["backups"] }))
        .catch((e) => reportProblem(`备份没成功：${errorText(e)}`));
    };
    window.addEventListener("focus", check);
    document.addEventListener("visibilitychange", check);
    return () => {
      window.removeEventListener("focus", check);
      document.removeEventListener("visibilitychange", check);
    };
  }, [enabled, qc]);
}

/** ⌘Z / ⌘⇧Z undo / redo the last edits (Rust keeps the history). Text fields keep their own undo. */
function useUndoKeys(enabled: boolean) {
  const qc = useQueryClient();
  const toast = useToast();
  useEffect(() => {
    if (!enabled) return;
    const onKey = async (e: KeyboardEvent) => {
      if (!e.metaKey || e.key.toLowerCase() !== "z" || e.altKey || e.ctrlKey) return;
      const el = document.activeElement as HTMLElement | null;
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) return;
      if (document.querySelector("dialog[open]")) return;
      e.preventDefault();
      const redo = e.shiftKey;
      try {
        const r = await (redo ? api.redo() : api.undo());
        if (!r.label) return toast(redo ? "没有可以重做的了" : "没有可以撤销的了");
        await qc.invalidateQueries();
        toast(`${redo ? "已重做" : "已撤销"}：${r.label}`);
      } catch (err) {
        toast(errorText(err), "error");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enabled, qc, toast]);
}

/** 芝麻开门 stays open only while the app is in use: activity anywhere keeps Rust's timer fresh (at most
 * every 20 s); 5 minutes without any input locks it (Rust also refuses an idle key on its own). */
function useVaultIdleLock(enabled: boolean) {
  const qc = useQueryClient();
  useEffect(() => {
    if (!enabled) return;
    const isOpen = () => qc.getQueryData<VaultStatus>(VAULT_STATUS_KEY)?.state === "open";
    let last = Date.now();
    let pinged = 0;
    const refresh = () => void qc.invalidateQueries({ queryKey: VAULT_KEY });
    const onActive = () => {
      last = Date.now();
      if (isOpen() && last - pinged > 20_000) {
        pinged = last;
        vaultApi
          .touch()
          .then((open) => !open && refresh())
          .catch(() => {});
      }
    };
    const tick = () => {
      if (isOpen() && Date.now() - last >= IDLE_MS) void vaultApi.lock().finally(refresh);
    };
    const events = ["pointerdown", "pointermove", "keydown", "wheel"] as const;
    events.forEach((e) => window.addEventListener(e, onActive, { passive: true }));
    const timer = setInterval(tick, 10_000);
    return () => {
      events.forEach((e) => window.removeEventListener(e, onActive));
      clearInterval(timer);
    };
  }, [enabled, qc]);
}

function StartupError({ info }: { info: AppInfo }) {
  return (
    <div data-tauri-drag-region className="flex h-full flex-col items-center justify-center gap-3 px-16 text-center">
      <TriangleAlert size={32} strokeWidth={1.5} className="text-destructive" />
      <p className="text-lg font-bold">数据库没能打开</p>
      <p className="selectable max-w-xl text-sm text-muted-foreground">{info.startupError}</p>
      <p className="selectable font-mono text-xs text-muted-foreground">{info.dataDir}</p>
      <Button onClick={() => api.revealDataDir()}>
        <FolderOpen size={14} /> 在 Finder 中打开数据目录
      </Button>
    </div>
  );
}

/** ⌘K opens 全局搜索 (not over another dialog). */
function useSearchKey(enabled: boolean, open: () => void) {
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || e.key.toLowerCase() !== "k" || e.shiftKey || e.altKey || e.ctrlKey) return;
      if (document.querySelector("dialog[open]")) return;
      e.preventDefault();
      open();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enabled, open]);
}

export function App() {
  return (
    <TabsProvider>
      <Shell />
    </TabsProvider>
  );
}

/** The frame (requirements「界面外框」, the B3 desk): the top bar, the desk with the open pages (首页 lies on it), the
 *  小窗, the 柚子瓣 and the 贴纸簿. */
function Shell() {
  const info = useQuery({ queryKey: ["app_info"], queryFn: api.appInfo });
  const ok = !!info.data && !info.data.startupError;
  const [searching, setSearching] = useState(false);
  const openSearch = useCallback(() => setSearching(true), []);
  useDailyChores(ok);
  useUndoKeys(ok);
  useVaultIdleLock(ok);
  useSearchKey(ok, openSearch);
  useJobWatch();
  useLookColors();
  // 柚子's 小窗: not on the 柚子 page, not when 柚子 is hidden
  const [hidden] = useHiddenModules();
  const path = useLocation().pathname;
  const chatUsable = ok && !hidden.includes("youzi") && !path.startsWith("/youzi");
  const [chatOpen, setChatOpen] = useState(false);
  const toggleChat = useCallback(() => setChatOpen((o) => !o), []);
  useChatKey(chatUsable, toggleChat);
  const floating = useMedia(FLOAT_QUERY);
  const published = usePublishedContext();
  const context = path.startsWith("/vault") ? null : (published ?? routeContext(path));
  const chat = chatUsable && chatOpen;
  const unseen = (useYouziStatus().data?.unseen ?? 0) > 0;
  /** The 柚子瓣, grown from the pomelo. */
  const [bloom, setBloom] = useState<HTMLElement | null>(null);
  /** A question from the 柚子瓣 for the 小窗. */
  const [ask, setAsk] = useState<{ text: string; n: number } | null>(null);
  const askChat = (text: string) => {
    setChatOpen(true);
    setAsk((a) => ({ text, n: (a?.n ?? 0) + 1 }));
  };
  useTabKeys(ok);
  useCapture(ok);
  const tabs = useTabs();
  // the frame's cues for the pomelo: 哇 when the 柚子瓣 / 贴纸簿 opens, 开心 when a page comes to the front, 眨眼 at the desk
  const [face, setFace] = useState<Face | null>(null);
  const faceT = useRef(0);
  const cue = useCallback((f: Face | null, ms: number) => {
    setFace(f);
    clearTimeout(faceT.current);
    faceT.current = window.setTimeout(() => setFace(null), ms);
  }, []);
  const frontId = tabs.stack[0]?.id ?? null;
  const firstCue = useRef(true);
  useEffect(() => {
    if (firstCue.current) {
      firstCue.current = false;
      return;
    }
    cue(frontId == null ? "wink" : "success", 900);
  }, [frontId, cue]);
  // a page can ask for a face too (the 今日简报's sticker falling back: 眨眼)
  useEffect(() => {
    const on = (e: Event) => {
      const d = (e as CustomEvent<{ face: Face; ms: number }>).detail;
      cue(d.face, d.ms);
    };
    window.addEventListener("pomelo:face", on);
    return () => window.removeEventListener("pomelo:face", on);
  }, [cue]);
  useEffect(() => {
    if (bloom || tabs.book) cue("wow", 900);
  }, [!!bloom, tabs.book]); // eslint-disable-line react-hooks/exhaustive-deps

  if (info.data?.startupError) return <StartupError info={info.data} />;
  return (
    <div className="flex h-full flex-col bg-sidebar">
      <TopBar
        face={face}
        isDev={info.data?.isDev}
        bloomOpen={!!bloom}
        onPomelo={(el) => setBloom((b) => (b ? null : el))}
        onSearch={ok ? openSearch : undefined}
        onChat={chatUsable ? toggleChat : undefined}
        chatOpen={chat}
        unseen={unseen}
      />
      <div className="relative flex min-h-0 flex-1">
        <div
          className="desk"
          // editors on the right edge make room for a floating 小窗
          style={chat && floating ? ({ "--chat-float": "380px" } as React.CSSProperties) : undefined}
        >
          {tabs.home ? (
            <div className="deskhome">
              <div className="deskpage">{ok && <Outlet />}</div>
              {ok && <DeskCommon />}
            </div>
          ) : (
            <PaperStack>{ok && <Outlet />}</PaperStack>
          )}
          {tabs.book && ok && <StickerBook onClose={() => tabs.setBook(false)} onAsk={ok && !hidden.includes("youzi") && !path.startsWith("/youzi") ? askChat : null} />}
        </div>
        {chat && (
          <Suspense fallback={null}>
            <SideChat context={context} floating={floating} ask={ask} onClose={() => setChatOpen(false)} />
          </Suspense>
        )}
        {bloom && ok && (
          <Bloom
            anchor={bloom}
            onClose={() => setBloom(null)}
            onAsk={
              ok && !hidden.includes("youzi") && !path.startsWith("/youzi")
                ? askChat
                : null
            }
          />
        )}
      </div>
      <DragChip />
      <CheerLayer />
      {searching && <SearchBox onClose={() => setSearching(false)} onAsk={ok && !hidden.includes("youzi") && !path.startsWith("/youzi") ? askChat : null} />}
    </div>
  );
}
