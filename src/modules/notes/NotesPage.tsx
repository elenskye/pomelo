// 念念不忘 (after Flomo): left column = counts · heatmap · 全部笔记 · tag tree; right = the box for a
// new note, then the stream (newest first; pinned on top when unfiltered). Filters (tag incl. sub-tags,
// day, words, one note) combine; clicking a #tag in a note or a day in the heatmap sets them.
import { useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { Notebook, Quote, Send, X } from "lucide-react";
import { moduleById } from "@/app/modules";
import { Heatmap } from "@/shared/charts";
import { localToday, slashDate } from "@/shared/dates";
import { errorText, useToast } from "@/shared/feedback";
import { usePageKeys } from "@/shared/panel";
import { Button, PageHeader, SearchInput } from "@/shared/ui";
import { useNoteMutations, useNotes, useOverview, type Brief, type Note, type NoteFilter } from "./api";
import { Editor, type EditorHandle } from "./Editor";
import { excerpt } from "./NoteBody";
import { BriefRow, NoteCard } from "./NoteCard";
import { TagTree } from "./TagTree";

type Filters = { tag: string | null; day: string | null; id: number | null };
const NONE: Filters = { tag: null, day: null, id: null };
const PAGE = 50;

function Chip(props: { children: ReactNode; onClear: () => void }) {
  return (
    <span className="inline-flex items-center gap-1 rounded-control bg-accent py-0.5 pr-1 pl-2 text-sm">
      {props.children}
      <button type="button" aria-label="取消筛选" onClick={props.onClear} className="rounded p-0.5 text-muted-foreground hover:bg-black/5 hover:text-foreground dark:hover:bg-white/10">
        <X size={12} />
      </button>
    </span>
  );
}

function Composer(props: { tags: string[]; activeTag: string | null; quoting: Brief[]; setQuoting: (b: Brief[]) => void; editor: RefObject<EditorHandle | null>; onOpen: (id: number) => void }) {
  const m = useNoteMutations();
  const toast = useToast();
  const [draft, setDraft] = useState("");
  const prefill = useRef("");
  // In a tag view a fresh note starts with that tag (an untouched prefill follows the view).
  useEffect(() => {
    const next = props.activeTag ? `#${props.activeTag} ` : "";
    setDraft((d) => (d === "" || d === prefill.current ? next : d));
    prefill.current = next;
  }, [props.activeTag]);
  const empty = !draft.trim() || draft === prefill.current;
  const submit = () => {
    if (empty || m.create.isPending) return;
    m.create.mutate(
      { content: draft, refs: props.quoting.map((b) => b.id) },
      {
        onSuccess: () => {
          setDraft(prefill.current);
          props.setQuoting([]);
        },
        onError: (e) => toast(errorText(e), "error"),
      },
    );
  };
  return (
    <div className="rounded-card border bg-card px-4 pt-3 pb-2 focus-within:ring-2 focus-within:ring-ring">
      <Editor ref={props.editor} value={draft} onChange={setDraft} onSubmit={submit} tags={props.tags} placeholder="现在的想法是……" />
      <div className="flex items-end gap-3 pt-1">
        <div className="min-w-0 flex-1 space-y-0.5">
          {props.quoting.map((b) => (
            <BriefRow key={b.id} brief={b} icon={<Quote size={12} />} onOpen={props.onOpen} onRemove={() => props.setQuoting(props.quoting.filter((x) => x.id !== b.id))} />
          ))}
        </div>
        <Button variant="primary" disabled={empty || m.create.isPending} onClick={submit} aria-label="记下">
          <Send size={14} /> 记下
        </Button>
      </div>
    </div>
  );
}

export default function NotesPage() {
  const mod = moduleById("notes");
  const today = localToday();
  const [f, setF] = useState<Filters>(NONE);
  const [search, setSearch] = useState("");
  const [q, setQ] = useState("");
  const [quoting, setQuoting] = useState<Brief[]>([]);
  const [shown, setShown] = useState(PAGE);
  const editor = useRef<EditorHandle>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const scroller = useRef<HTMLElement>(null);
  const sentinel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const t = setTimeout(() => setQ(search.trim()), 200);
    return () => clearTimeout(t);
  }, [search]);

  const filter: NoteFilter = useMemo(() => {
    const out: NoteFilter = {};
    if (f.id !== null) out.id = f.id;
    if (f.tag) out.tag = f.tag;
    if (f.day) out.day = f.day;
    if (q) out.q = q;
    return out;
  }, [f, q]);
  const notesQ = useNotes(filter);
  const notes = notesQ.data ?? [];
  const overview = useOverview().data;
  const tagNames = useMemo(() => (overview?.tags ?? []).map((t) => t.tag), [overview]);
  const dayCounts = useMemo(() => new Map((overview?.days ?? []).map((d) => [d.day, d.count])), [overview]);
  const filtered = f.tag !== null || f.day !== null || f.id !== null || q !== "";

  // new filter → back to the top, first page
  useEffect(() => {
    setShown(PAGE);
    scroller.current?.scrollTo({ top: 0 });
  }, [filter]);
  useEffect(() => {
    const el = sentinel.current;
    if (!el) return;
    const io = new IntersectionObserver(([e]) => e.isIntersecting && setShown((n) => n + PAGE), { root: scroller.current, rootMargin: "400px" });
    io.observe(el);
    return () => io.disconnect();
  }, [notes.length, shown]);

  const go = (next: Partial<Filters>) => setF({ ...f, id: null, ...next });
  const openNote = (id: number) => {
    setSearch("");
    setQ("");
    setF({ ...NONE, id });
  };
  const quote = (n: Note) => {
    const b: Brief = { id: n.id, createdAt: n.createdAt, excerpt: excerpt(n.content) };
    if (!quoting.some((x) => x.id === n.id)) setQuoting([...quoting, b]);
    scroller.current?.scrollTo({ top: 0, behavior: "smooth" });
    editor.current?.focus();
  };

  usePageKeys({
    onNew: () => editor.current?.focus(),
    onFind: () => searchRef.current?.focus(),
    onEscape: f.id !== null ? () => setF(NONE) : undefined,
  });

  const stat = (n: number | undefined, label: string) => (
    <div className="flex flex-col">
      <span className="text-xl font-bold tabular-nums">{n ?? "–"}</span>
      <span className="text-xs text-muted-foreground">{label}</span>
    </div>
  );

  const nav = (
    <nav aria-label={mod.label} className="flex w-60 shrink-0 flex-col overflow-y-auto pr-1">
      <div className="grid grid-cols-3 px-2.5 pb-3">
        {stat(overview?.total, "笔记")}
        {stat(overview?.tags.length, "标签")}
        {stat(overview?.days.length, "天")}
      </div>
      <div className="px-2.5 pb-4">
        <Heatmap
          counts={dayCounts}
          today={today}
          selected={f.day}
          onSelect={(d) => go({ day: f.day === d ? null : d })}
          title={(d, n) => `${slashDate(d)} · ${n} 条`}
        />
      </div>
      <button
        type="button"
        aria-current={f.tag === null}
        onClick={() => go({ tag: null })}
        className={`flex items-center gap-2 rounded-control px-2.5 py-1.5 text-left outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring ${
          f.tag === null ? "bg-accent" : "hover:bg-muted/70"
        }`}
      >
        <Notebook size={15} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
        <span className="flex-1">全部笔记</span>
        <span className="text-xs text-muted-foreground tabular-nums">{overview?.total ?? ""}</span>
      </button>
      {tagNames.length > 0 && (
        <>
          <div className="px-2.5 pt-4 pb-1 text-xs text-muted-foreground">标签</div>
          <TagTree tags={overview?.tags ?? []} active={f.tag} onSelect={(tag) => go({ tag })} />
        </>
      )}
    </nav>
  );

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader
        icon={mod.icon}
        color={mod.color}
        title={mod.label}
        right={<SearchInput ref={searchRef} aria-label="搜索笔记" placeholder="搜索" value={search} onChange={(e) => setSearch(e.target.value)} className="w-56" />}
      />
      <div className="flex min-h-0 flex-1 gap-6 px-8 pb-6">
        {nav}
        <section ref={scroller} className="min-w-0 flex-1 overflow-y-auto">
          <div className="max-w-3xl space-y-3 pb-8">
            <Composer tags={tagNames} activeTag={f.tag} quoting={quoting} setQuoting={setQuoting} editor={editor} onOpen={openNote} />
            {filtered && (
              <div className="flex flex-wrap items-center gap-2 pt-1">
                {f.id !== null && <Chip onClear={() => setF(NONE)}>这条笔记</Chip>}
                {f.tag && <Chip onClear={() => go({ tag: null })}>#{f.tag}</Chip>}
                {f.day && <Chip onClear={() => go({ day: null })}>{slashDate(f.day)}</Chip>}
                {q && <Chip onClear={() => setSearch("")}>「{q}」</Chip>}
                {notesQ.data && <span className="text-sm text-muted-foreground">{notes.length} 条</span>}
              </div>
            )}
            {notes.slice(0, shown).map((n) => (
              <NoteCard
                key={`${n.id}${f.id === n.id ? "f" : ""}`}
                note={n}
                tags={tagNames}
                focused={f.id === n.id}
                onTag={(tag) => go({ tag })}
                onOpen={openNote}
                onQuote={quote}
              />
            ))}
            {shown < notes.length && <div ref={sentinel} className="h-8" />}
            {notesQ.data && notes.length === 0 && (
              <p className="py-12 text-center text-muted-foreground">{overview?.total === 0 && !filtered ? "念念不忘，必有回响" : "没有找到"}</p>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
