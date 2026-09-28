import { useEffect, useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { FolderOpen, TriangleAlert } from "lucide-react";
import { api, SETTING_LAST_ROUTE, type AppInfo } from "@/shared/ipc";
import { Button } from "@/shared/ui";
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

  if (info.data?.startupError) return <StartupError info={info.data} />;
  return (
    <div className="flex h-full">
      <Sidebar isDev={info.data?.isDev} />
      <main className="flex min-w-0 flex-1 flex-col overflow-y-auto">{ok && <Outlet />}</main>
    </div>
  );
}
