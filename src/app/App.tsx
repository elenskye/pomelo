import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { Outlet, useLocation } from "react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderOpen, TriangleAlert } from "lucide-react";
import { errorText, useToast } from "@/shared/feedback";
import { api, type AppInfo } from "@/shared/ipc";
import { Button } from "@/shared/ui";
import { IDLE_MS, VAULT_KEY, VAULT_STATUS_KEY, vaultApi, type VaultStatus } from "@/modules/vault/api";
import { useHiddenModules } from "./hidden";
import { usePublishedContext, type PageContext } from "./pageContext";
import { SearchBox } from "./Search";
import { Sidebar } from "./Sidebar";

// 小拾's 小窗 brings the Markdown / formula libraries: loaded the first time it opens
const SideChat = lazy(() => import("@/modules/picky/SideChat").then((m) => ({ default: m.SideChat })));
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
        .catch(() => {});
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
  const info = useQuery({ queryKey: ["app_info"], queryFn: api.appInfo });
  const ok = !!info.data && !info.data.startupError;
  const [searching, setSearching] = useState(false);
  const openSearch = useCallback(() => setSearching(true), []);
  useDailyChores(ok);
  useUndoKeys(ok);
  useVaultIdleLock(ok);
  useSearchKey(ok, openSearch);
  // 小拾's 小窗: not on the 小拾 page, not when 小拾 is hidden
  const [hidden] = useHiddenModules();
  const path = useLocation().pathname;
  const chatUsable = ok && !hidden.includes("picky") && !path.startsWith("/picky");
  const [chatOpen, setChatOpen] = useState(false);
  const toggleChat = useCallback(() => setChatOpen((o) => !o), []);
  useChatKey(chatUsable, toggleChat);
  const floating = useMedia(FLOAT_QUERY);
  const published = usePublishedContext();
  const context = path.startsWith("/vault") ? null : (published ?? routeContext(path));
  const chat = chatUsable && chatOpen;

  if (info.data?.startupError) return <StartupError info={info.data} />;
  return (
    <div className="relative flex h-full">
      <Sidebar isDev={info.data?.isDev} onSearch={ok ? openSearch : undefined} onChat={chatUsable ? toggleChat : undefined} chatOpen={chat} />
      <main
        className="relative flex min-w-0 flex-1 flex-col overflow-y-auto"
        // editors on the right edge make room for a floating 小窗
        style={chat && floating ? ({ "--chat-float": "380px" } as React.CSSProperties) : undefined}
      >
        {ok && <Outlet />}
      </main>
      {chat && (
        <Suspense fallback={null}>
          <SideChat context={context} floating={floating} onClose={() => setChatOpen(false)} />
        </Suspense>
      )}
      {searching && <SearchBox onClose={() => setSearching(false)} />}
    </div>
  );
}
