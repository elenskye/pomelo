// 全局搜索: ⌘K (or the sidebar's magnifier) opens a box over every module that isn't hidden (芝麻开门 is
// never searched — Rust doesn't look there). Words all have to match (search/mod.rs). Results are grouped
// by module; below them 意思相近 (v0.4: by meaning, a moment later, without what words found). ↑↓ move,
// ↩ opens (navigate with the hit; the page opens the record), Esc closes.
import { useEffect, useMemo, useRef, useState } from "react";
import { useTabs } from "./tabs";
import { useQuery, keepPreviousData } from "@tanstack/react-query";
import { Search as SearchIcon, Sparkles } from "lucide-react";
import { errorText } from "@/shared/feedback";
import { money, type Currency } from "@/modules/finance/money";
import { slashDate } from "@/shared/dates";
import { searchAll, searchSimilar, type Hit } from "@/shared/open";
import { useAllModules } from "@/modules/custom/api";
import { useHiddenModules } from "./hidden";

const md = (d: string) => `${Number(d.slice(5, 7))}/${Number(d.slice(8))}`;

/** The muted text after a title. */
function detail(h: Hit): string | null {
  if (h.kind === "entry" && h.day && h.amount !== null && h.currency)
    return [md(h.day), money(h.amount, h.currency as Currency, h.flow === "in"), h.detail].filter(Boolean).join(" · ");
  if (h.kind === "note" && h.day) return slashDate(h.day);
  if (h.kind === "task" && h.day) return `${h.detail} · ${slashDate(h.day)}`;
  return h.detail;
}

export function SearchBox(props: { onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const tabs = useTabs();
  const [hidden] = useHiddenModules();
  const modules = useAllModules();
  const [text, setText] = useState("");
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  useEffect(() => {
    const t = setTimeout(() => setQ(text.trim()), 120);
    return () => clearTimeout(t);
  }, [text]);
  const { data = [], isFetched } = useQuery({ queryKey: ["search", q], queryFn: () => searchAll(q), enabled: q !== "", placeholderData: keepPreviousData });
  const similar = useQuery({ queryKey: ["search", "similar", q], queryFn: () => searchSimilar(q), enabled: q !== "", retry: false, staleTime: 30_000 });
  // grouped in the sidebar's module order (the list below and the arrow keys go the same way)
  const hits = useMemo(() => {
    const rank = (h: Hit) => modules.findIndex((m) => m.id === h.module);
    return q ? data.filter((h) => !hidden.includes(h.module) && rank(h) >= 0).sort((a, b) => rank(a) - rank(b)) : [];
  }, [data, hidden, q, modules]);
  const near = useMemo(
    () => (q && similar.data ? similar.data.filter((h) => !hidden.includes(h.module) && !hits.some((x) => x.module === h.module && x.kind === h.kind && x.id === h.id)) : []),
    [similar.data, hidden, hits, q],
  );
  const all = useMemo(() => [...hits, ...near], [hits, near]);
  useEffect(() => setActive(0), [hits]);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const open = (h: Hit) => {
    props.onClose();
    tabs.open(`/${h.module}`, { open: h });
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (all.length) setActive((i) => (i + (e.key === "ArrowDown" ? 1 : -1) + all.length) % all.length);
    } else if (e.key === "Enter" && !e.nativeEvent.isComposing) {
      e.preventDefault();
      if (all[active]) open(all[active]);
    }
  };

  let i = -1;
  return (
    <dialog
      ref={ref}
      aria-label="全局搜索"
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      onClick={(e) => e.target === ref.current && props.onClose()}
      className="mx-auto mt-[12vh] w-[600px] max-w-[calc(100vw-32px)] overflow-hidden rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <div className="flex items-center gap-2.5 border-b px-4">
        <SearchIcon size={16} className="shrink-0 text-muted-foreground" />
        <input
          autoFocus
          aria-label="搜索全部"
          placeholder="搜索"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKey}
          className="h-12 min-w-0 flex-1 bg-transparent text-base outline-none placeholder:text-muted-foreground"
        />
      </div>
      {q !== "" && (
        <div ref={list} role="listbox" aria-label="搜索结果" className="max-h-[56vh] overflow-y-auto p-1.5">
          {all.length === 0 && !similar.isFetching ? (
            isFetched && <p className="px-3 py-6 text-center text-sm text-muted-foreground">没有找到</p>
          ) : (
            modules.filter((m) => hits.some((h) => h.module === m.id)).map((m) => {
              const Icon = m.icon;
              return (
                <div key={m.id} className="pb-1">
                  <div className="flex items-center gap-1.5 px-2.5 pt-2 pb-1 text-xs text-muted-foreground">
                    <Icon size={13} strokeWidth={1.75} style={{ color: m.color }} />
                    {m.label}
                  </div>
                  {hits
                    .filter((h) => h.module === m.id)
                    .map((h) => {
                      i += 1;
                      const index = i;
                      const d = detail(h);
                      return (
                        <div
                          key={`${h.kind}${h.id}`}
                          role="option"
                          aria-selected={index === active}
                          data-index={index}
                          onMouseMove={() => setActive(index)}
                          onClick={() => open(h)}
                          className={`flex cursor-default items-baseline gap-3 rounded-control px-2.5 py-1.5 text-sm ${index === active ? "bg-accent" : ""}`}
                        >
                          <span className="min-w-0 flex-1 truncate">{h.title}</span>
                          {d && <span className="max-w-[45%] shrink-0 truncate text-xs text-muted-foreground tabular-nums">{d}</span>}
                        </div>
                      );
                    })}
                </div>
              );
            })
          )}
          {(near.length > 0 || similar.isFetching || similar.isError) && (
            <div className="pb-1">
              <div className="flex items-center gap-1.5 px-2.5 pt-2 pb-1 text-xs text-muted-foreground">
                <Sparkles size={13} strokeWidth={1.75} />
                意思相近
                {similar.isFetching && near.length === 0 && <span className="animate-pulse">· 正在找…</span>}
                {similar.isError && <span className="truncate">· {errorText(similar.error)}</span>}
              </div>
              {near.map((h) => {
                i += 1;
                const index = i;
                const d = detail(h);
                const m = modules.find((x) => x.id === h.module);
                const Icon = m?.icon;
                return (
                  <div
                    key={`near-${h.module}${h.kind}${h.id}`}
                    role="option"
                    aria-selected={index === active}
                    data-index={index}
                    onMouseMove={() => setActive(index)}
                    onClick={() => open(h)}
                    className={`flex cursor-default items-baseline gap-2 rounded-control px-2.5 py-1.5 text-sm ${index === active ? "bg-accent" : ""}`}
                  >
                    {Icon && <Icon size={13} strokeWidth={1.75} className="shrink-0 self-center" style={{ color: m.color }} aria-label={m.label} />}
                    <span className="min-w-0 flex-1 truncate">{h.title}</span>
                    {d && <span className="max-w-[45%] shrink-0 truncate text-xs text-muted-foreground tabular-nums">{d}</span>}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}
    </dialog>
  );
}
