// Modules hidden from the sidebar (and from 全局搜索); their data stays. One query so the sidebar, the
// settings page and search see a change at once.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/shared/ipc";
import { isStringArray } from "@/shared/settings";

const KEY = "ui.module_hidden";
const QUERY = ["setting", KEY];

export function useHiddenModules() {
  const qc = useQueryClient();
  const { data = [] } = useQuery({ queryKey: QUERY, queryFn: () => api.settingsGet<unknown>(KEY).then((v) => (isStringArray(v) ? v : [])) });
  const set = (ids: string[]) => {
    qc.setQueryData(QUERY, ids);
    api.settingsSet(KEY, ids).catch(() => {});
  };
  return [data, set] as const;
}
