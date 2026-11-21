// What every module wears, settled across all of them (looks.ts effectiveLooks: no two alike), and the colors
// that come of it on <html>. Kept apart from looks.ts because it needs the made modules (modules/custom/api).
import { useEffect, useMemo, useRef } from "react";
import { useAllModules } from "@/modules/custom/api";
import { tintColor } from "@/shared/marks";
import { colorVar, effectiveLooks, lookKey, ownTint, useLooks, type Effective } from "./looks";
import { ASSISTANT_MODULES, type ModuleDef } from "./modules";

/** Every module (built-in, made, 助手), and what each one wears. */
export function useModuleLooks() {
  const { looks } = useLooks();
  const all = useAllModules();
  return useMemo(() => {
    const defs: ModuleDef[] = [...all, ...ASSISTANT_MODULES];
    return { defs, eff: effectiveLooks(defs, looks) };
  }, [all, looks]);
}

/** What the others wear (lookKey → their name), so `except` can't pick it too. */
export function takenBy(defs: ModuleDef[], eff: Map<string, Effective>, except: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const d of defs) {
    const e = eff.get(d.id);
    if (e && d.id !== except) out.set(lookKey(e.mark, e.tint), d.label);
  }
  return out;
}

/** Keeps each module's color variable on <html> while it wears another color than its own. Mounted once in App. */
export function useLookColors() {
  const { defs, eff } = useModuleLooks();
  const set = useRef<string[]>([]);
  useEffect(() => {
    const root = document.documentElement.style;
    for (const v of set.current) root.removeProperty(v);
    set.current = [];
    for (const d of defs) {
      const e = eff.get(d.id);
      if (!e?.tint || e.tint === ownTint(d)) continue;
      root.setProperty(colorVar(d.id), tintColor(e.tint));
      set.current.push(colorVar(d.id));
    }
  }, [defs, eff]);
}
