// 小拾's own search (the magnifier above the conversation list): titles and every message, all words
// must appear; filters for the reply's model, a time range and whether archived ones count. Opening a
// result jumps to that message (switching to its version if it's an older one). Rules in find.rs.
// Below: 意思相近 (v0.4, by meaning, same filters, without the conversations words found).
import { useEffect, useRef, useState } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { Search, Sparkles, X } from "lucide-react";
import { errorText } from "@/shared/feedback";
import { Dropdown } from "@/shared/Dropdown";
import { Switch } from "@/shared/ui";
import { pickyApi, type ChatHit } from "./api";

const RANGES = [
  { value: 0, label: "全部时间" },
  { value: 7, label: "7 天内" },
  { value: 30, label: "30 天内" },
  { value: 365, label: "一年内" },
];

const day = (iso: string) => {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso.slice(0, 10) : `${d.getFullYear()} / ${String(d.getMonth() + 1).padStart(2, "0")} / ${String(d.getDate()).padStart(2, "0")}`;
};

/** The snippet with the query words emphasised. */
function Marked(props: { text: string; words: string[] }) {
  const ws = props.words.filter(Boolean);
  if (!ws.length) return <>{props.text}</>;
  const re = new RegExp(`(${ws.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`, "gi");
  return (
    <>
      {props.text.split(re).map((part, i) =>
        i % 2 === 1 ? (
          <span key={i} className="text-foreground">
            {part}
          </span>
        ) : (
          part
        ),
      )}
    </>
  );
}

export function ChatSearch(props: { onOpen: (hit: ChatHit, query: string) => void; onClose: () => void }) {
  const [text, setText] = useState("");
  const [q, setQ] = useState("");
  const [model, setModel] = useState<string | null>(null);
  const [days, setDays] = useState(0);
  const [archived, setArchived] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  useEffect(() => {
    const t = setTimeout(() => setQ(text.trim()), 150);
    return () => clearTimeout(t);
  }, [text]);
  const models = useQuery({ queryKey: ["picky", "search-models"], queryFn: pickyApi.searchModels }).data ?? [];
  const since = days ? new Date(Date.now() - days * 86400000).toISOString() : null;
  const hits = useQuery({
    queryKey: ["picky", "search", q, model, days, archived],
    queryFn: () => pickyApi.search(q, model, since, archived),
    enabled: q !== "",
    placeholderData: keepPreviousData,
  });
  const similar = useQuery({
    queryKey: ["picky", "search-similar", q, model, days, archived],
    queryFn: () => pickyApi.searchSimilar(q, model, since, archived),
    enabled: q !== "",
    retry: false,
    staleTime: 30_000,
  });
  const words = q.split(/\s+/);
  const results = q ? (hits.data ?? []) : [];
  const near = q ? (similar.data ?? []).filter((h) => !results.some((r) => r.sessionId === h.sessionId)) : [];
  const row = (h: ChatHit, marked: boolean) => (
    <button
      key={`${h.sessionId}-${h.messageId}`}
      type="button"
      onClick={() => props.onOpen(h, q)}
      className="block w-full rounded-control px-2.5 py-2 text-left outline-none hover:bg-muted/70 focus-visible:ring-2 focus-visible:ring-ring"
    >
      <span className="block truncate">{h.title}</span>
      {h.snippet && (
        <span className="mt-0.5 line-clamp-2 block font-reading text-xs leading-relaxed text-muted-foreground">
          {marked ? <Marked text={h.snippet} words={words} /> : h.snippet}
        </span>
      )}
      <span className="mt-0.5 block text-xs text-muted-foreground">
        {[day(h.at), h.model, h.archived ? "已归档" : null].filter(Boolean).join(" · ")}
      </span>
    </button>
  );

  return (
    <div className="-m-1 flex w-[248px] shrink-0 flex-col p-1 pr-2">
      <div className="flex items-center gap-1.5 rounded-control border bg-card px-2.5 py-1.5 focus-within:ring-2 focus-within:ring-ring">
        <Search size={14} className="shrink-0 text-muted-foreground" />
        <input
          ref={input}
          value={text}
          spellCheck={false}
          aria-label="搜索对话"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              props.onClose();
            } else if (e.key === "Enter" && !e.nativeEvent.isComposing && results[0]) props.onOpen(results[0], q);
          }}
          className="min-w-0 flex-1 bg-transparent text-sm outline-none"
        />
        <button type="button" aria-label="关闭搜索" onClick={props.onClose} className="rounded text-muted-foreground hover:text-foreground">
          <X size={14} />
        </button>
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-1.5 text-xs">
        <Dropdown<string | null>
          ariaLabel="模型"
          value={model}
          options={[{ value: null, label: "全部模型" }, ...models.map((m) => ({ value: m, label: m }))]}
          onChange={setModel}
          triggerClassName="inline-flex items-center gap-1 rounded-control border bg-card px-2 py-1 text-xs outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
        />
        <Dropdown<number>
          ariaLabel="时间"
          value={days}
          options={RANGES}
          onChange={setDays}
          triggerClassName="inline-flex items-center gap-1 rounded-control border bg-card px-2 py-1 text-xs outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
        />
        <span className="ml-auto flex items-center gap-1.5 text-muted-foreground">
          含归档
          <Switch checked={archived} onChange={setArchived} label="含归档" />
        </span>
      </div>
      <div className="-m-1 mt-2 min-h-0 flex-1 overflow-y-auto p-1">
        {q && hits.isFetched && results.length === 0 && near.length === 0 && !similar.isFetching && <p className="px-2.5 py-4 text-center text-sm text-muted-foreground">没找到</p>}
        {results.map((h) => row(h, true))}
        {q && (near.length > 0 || similar.isFetching || similar.isError) && (
          <>
            <p className="flex items-center gap-1.5 px-2.5 pt-3 pb-1 text-xs text-muted-foreground">
              <Sparkles size={13} strokeWidth={1.75} />
              意思相近
              {similar.isFetching && near.length === 0 && <span className="animate-pulse">· 正在找…</span>}
              {similar.isError && <span className="truncate">· {errorText(similar.error)}</span>}
            </p>
            {near.map((h) => row(h, false))}
          </>
        )}
      </div>
    </div>
  );
}
