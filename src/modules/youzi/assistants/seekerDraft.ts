// One current draft across assistant mounts. Pending text survives page navigation and saves in order.
import { useSyncExternalStore } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { errorText } from "@/shared/feedback";
import { SEEKER_KEY, seekerApi, type SeekerDraft } from "./seekerApi";
let snapshot: { draft: SeekerDraft | null; saving: boolean; error: string | null } = { draft: null, saving: false, error: null };
let pending: SeekerDraft | null = null;
let running: Promise<boolean> | null = null;
let timer = 0;
const listeners = new Set<() => void>();
const publish = (next: typeof snapshot) => { snapshot = next; listeners.forEach((f) => f()); };
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
export const useSeekerDraft = () => useSyncExternalStore(subscribe, () => snapshot);
export function queueSeekerDraft(draft: SeekerDraft, qc: QueryClient) {
  pending = draft;
  publish({ draft, saving: true, error: null });
  window.clearTimeout(timer);
  timer = window.setTimeout(() => void flushSeekerDraft(qc), 450);
}
export function flushSeekerDraft(qc: QueryClient): Promise<boolean> {
  window.clearTimeout(timer);
  if (running) return running;
  if (!pending) return Promise.resolve(true);
  running = (async () => {
    while (pending) {
      const draft = pending;
      pending = null;
      try {
        const result = await seekerApi.save(draft);
        qc.setQueryData(SEEKER_KEY, result);
      } catch (e) {
        pending = pending ?? draft;
        publish({ draft: pending, saving: false, error: errorText(e) });
        return false;
      }
    }
    publish({ draft: null, saving: false, error: null });
    return true;
  })().finally(() => { running = null; });
  return running;
}
