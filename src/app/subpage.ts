// A module's pages live in the path (`/site/works`, modules.ts `subs`): each change is a 前进后退 step and
// names the 页签 (「呦呦漫游 · 作品」). A bare `/site` shows the page open last (this session; with
// `remember`, a settings key, also after a restart) and settles the 页签 on it.
import { useCallback, useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { api } from "@/shared/ipc";
import { moduleById, splitPath } from "./modules";
import { useTabs } from "./tabs";

const last = new Map<string, string>();

export function useSubPage<T extends string>(module: string, remember?: string): [T, (page: T) => void] {
  const pages = (moduleById(module).subs ?? []).map((s) => s.value);
  const valid = (v: unknown): v is T => typeof v === "string" && pages.includes(v);
  const location = useLocation();
  const navigate = useNavigate();
  const tabs = useTabs();
  const param = splitPath(location.pathname).sub;
  const [stored, setStored] = useState<{ value: T | null } | null>(remember ? null : { value: null });
  useEffect(() => {
    if (!remember) return;
    let live = true;
    api
      .settingsGet<unknown>(remember)
      .then((v) => live && setStored({ value: valid(v) ? v : null }))
      .catch(() => live && setStored({ value: null }));
    return () => {
      live = false;
    };
  }, [remember]); // eslint-disable-line react-hooks/exhaustive-deps
  const kept = last.get(module);
  const sub = (valid(param) ? param : valid(kept) ? kept : (stored?.value ?? pages[0])) as T;
  const ready = valid(param) || valid(kept) || stored !== null;
  useEffect(() => {
    if (!ready) return;
    last.set(module, sub);
    if (!valid(param)) tabs.settle(`/${module}/${sub}`);
  }, [module, sub, param, ready, location.key]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = useCallback(
    (page: T) => {
      if (page === sub) return;
      if (!valid(param)) tabs.settle(`/${module}/${sub}`);
      last.set(module, page);
      if (remember) api.settingsSet(remember, page).catch(() => {});
      void navigate(`/${module}/${page}`);
    },
    [sub, param, module, remember, navigate, tabs], // eslint-disable-line react-hooks/exhaustive-deps
  );
  return [sub, set];
}
