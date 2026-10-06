import { useEffect, useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderOpen, TriangleAlert } from "lucide-react";
import { errorText, useToast } from "@/shared/feedback";
import { api, SETTING_LAST_ROUTE, type AppInfo } from "@/shared/ipc";
import { Button } from "@/shared/ui";
import { IDLE_MS, VAULT_KEY, VAULT_STATUS_KEY, vaultApi, type VaultStatus } from "@/modules/vault/api";
import { Sidebar } from "./Sidebar";
import { DEFAULT_ROUTE, MODULES } from "./modules";

const KNOWN_ROUTES = [...MODULES.map((m) => `/${m.id}`), "/settings"];

/** Opens the last visited page on launch and remembers navigation. */
function useRouteMemory(enabled: boolean) {
  const location = useLocation();
  const navigate = useNavigate();
  const [restored, setRestored] = useState(false);

  useEffect(() => {
    if (!enabled || restored) return;
    api
      .settingsGet<string>(SETTING_LAST_ROUTE)
      .catch(() => null)
      .then((last) => {
        if (location.pathname === "/") {
          navigate(last && KNOWN_ROUTES.includes(last) ? last : DEFAULT_ROUTE, { replace: true });
        }
        setRestored(true);
      });
  }, [enabled, restored, location.pathname, navigate]);

  useEffect(() => {
    if (restored && KNOWN_ROUTES.includes(location.pathname)) {
      api.settingsSet(SETTING_LAST_ROUTE, location.pathname).catch(() => {});
    }
  }, [restored, location.pathname]);
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

export function App() {
  const info = useQuery({ queryKey: ["app_info"], queryFn: api.appInfo });
  const ok = !!info.data && !info.data.startupError;
  useRouteMemory(ok);
  useDailyChores(ok);
  useUndoKeys(ok);
  useVaultIdleLock(ok);

  if (info.data?.startupError) return <StartupError info={info.data} />;
  return (
    <div className="flex h-full">
      <Sidebar isDev={info.data?.isDev} />
      <main className="flex min-w-0 flex-1 flex-col overflow-y-auto">{ok && <Outlet />}</main>
    </div>
  );
}
