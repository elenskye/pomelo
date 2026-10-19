// What the page tells the 小拾 小窗 (requirements「小拾 · 懂 Pomelo」→ 当前页面): the module, its view, the
// filters as shown, the record whose editor is open, the records picked in 多选 — ids and labels only;
// Rust fetches the content under its rules. The innermost view publishes (usePageContext); a module
// that publishes nothing still counts by its route (App passes the fallback).
import { useEffect, useRef, useSyncExternalStore } from "react";
import type { PageContextPayload } from "@/modules/picky/api";

export type PageContext = {
  module: string;
  /** The view inside the module (收支, 统计…). */
  page?: string | null;
  /** Filters as shown (2026 年 9 月 · 餐饮). */
  filter?: string | null;
  /** The record whose editor is open; `label` is how the pill shows it (投递：某公司 · 产品经理). */
  record?: { kind: string; id: number; label: string } | null;
  /** Records picked in 多选 (same kind). */
  selected?: { kind: string; ids: number[]; label: string } | null;
};

let current: { owner: object; ctx: PageContext } | null = null;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

/** Publishes this view's context while it is mounted (null = nothing to say). */
export function usePageContext(ctx: PageContext | null) {
  const owner = useRef({}).current;
  const key = JSON.stringify(ctx);
  useEffect(() => {
    current = ctx ? { owner, ctx: JSON.parse(key) as PageContext } : null;
    emit();
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(
    () => () => {
      if (current?.owner === owner) {
        current = null;
        emit();
      }
    },
    [owner],
  );
}

export function usePublishedContext(): PageContext | null {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => current?.ctx ?? null,
  );
}

/** What goes to Rust. */
export const payload = (c: PageContext): PageContextPayload => ({
  module: c.module,
  page: c.page ?? null,
  filter: c.filter ?? null,
  record: c.record ? { kind: c.record.kind, id: c.record.id } : null,
  selected: c.selected ? c.selected.ids.map((id) => ({ kind: c.selected!.kind, id })) : [],
});

/** The pill: the open record, else the picked ones, else where (and filters). */
export const pillText = (c: PageContext, moduleLabel: string) =>
  c.record?.label ?? c.selected?.label ?? [moduleLabel, c.page, c.filter].filter(Boolean).join(" · ");
