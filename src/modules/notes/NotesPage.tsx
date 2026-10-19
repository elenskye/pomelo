// 念念不忘 (after Flomo): left column = counts · heatmap · 全部笔记 · pinned tags · tag tree; right = the box for a
// new note, then the stream (newest first; pinned on top when unfiltered). Filters (tag incl. sub-tags,
// day, words, one note) combine; clicking a #tag in a note or a day in the heatmap sets them. While
// searching, 意思相近 follows the word results (v0.4: by meaning, within the tag / day filters).
// 转成待办 opens 一步一步's new-task panel right here (title = the first line, 内容 = the whole note).
import { useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useGoals, useGroups } from "@/modules/todo/api";
import { TaskPanel } from "@/modules/todo/TaskPanel";
import { Notebook, Quote, Send, Sparkles, X } from "lucide-react";
import { moduleById } from "@/app/modules";
import type { FileInfo } from "@/shared/attachments/api";
import { AttachButton, AttachmentStrip, withFiles } from "@/shared/attachments/Attachments";
import { useAttachTarget } from "@/shared/attachments/target";
import { Heatmap } from "@/shared/charts";
import { localToday, slashDate } from "@/shared/dates";
import { errorText, useToast } from "@/shared/feedback";
import { useOpenRequest } from "@/shared/open";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { Button, PageHeader, SearchInput } from "@/shared/ui";
import { NOTES_KEY, useNoteMutations, useNotes, useOverview, useSimilarNotes, type Brief, type Note, type NoteFilter } from "./api";
import { Editor, type EditorHandle } from "./Editor";
import { excerpt } from "./NoteBody";
import { BriefRow, NoteCard } from "./NoteCard";
import { TagTree } from "./TagTree";
import { usePageContext } from "@/app/pageContext";

/** A task title from a note: its first line, without list marks, bold / highlight marks and #tags. */
export function todoTitle(content: string): string {
  const line = content.split("\n").map((l) => l.trim()).find((l) => l.replace(/#[^\s#]+/g, "").trim()) ?? "";
  const t = line
    .replace(/^(-|\d+\.)\s+/, "")
    .replace(/\*\*|==|\+\+/g, "")
    .replace(/(^|\s)#[^\s#]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  return [...t].slice(0, 100).join("");
}

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
  const [files, setFiles] = useState<FileInfo[]>([]);
  const box = useRef<HTMLDivElement>(null);
  const dropping = useAttachTarget(box, (fs) => setFiles((have) => withFiles(have, fs)), (e) => toast(errorText(e), "error"));
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
      { content: draft, refs: props.quoting.map((b) => b.id), attachments: files.map((f) => f.id) },
      {
        onSuccess: () => {
          setDraft(prefill.current);
          setFiles([]);
          props.setQuoting([]);
        },
        onError: (e) => toast(errorText(e), "error"),
      },
    );
  };
  return (
    <div ref={box} className={`rounded-card border bg-card px-4 pt-3 pb-2 focus-within:ring-2 focus-within:ring-ring ${dropping ? "ring-2 ring-primary" : ""}`}>
      <Editor ref={props.editor} value={draft} onChange={setDraft} onSubmit={submit} tags={props.tags} placeholder="现在的想法是……" />
      <AttachmentStrip files={files} onRemove={(id) => setFiles(files.filter((f) => f.id !== id))} className="pt-2" />
      <div className="flex items-end gap-3 pt-1">
        <div className="min-w-0 flex-1 space-y-0.5">
          {props.quoting.map((b) => (
            <BriefRow key={b.id} brief={b} icon={<Quote size={12} />} onOpen={props.onOpen} onRemove={() => props.setQuoting(props.quoting.filter((x) => x.id !== b.id))} />
          ))}
        </div>
        <AttachButton onFiles={(fs) => setFiles((have) => withFiles(have, fs))} />
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
  const similar = useSimilarNotes(filter, q !== "" && f.id === null);
  const near = q && f.id === null ? (similar.data ?? []).filter((n) => !notes.some((x) => x.id === n.id)) : [];
  const nearShown = q !== "" && f.id === null && (near.length > 0 || similar.isFetching || similar.isError);
  const overview = useOverview().data;
  const qc = useQueryClient();
  const { data: groups = [] } = useGroups();
  const { data: goals = [] } = useGoals();
  const toTodo = usePanelSelection<number>();
  const converting = toTodo.selection !== null ? (notes.find((x) => x.id === toTodo.selection) ?? null) : null;
  const tagNames = useMemo(() => (overview?.tags ?? []).map((t) => t.tag), [overview]);
  const dayCounts = useMemo(() => new Map((overview?.days ?? []).map((d) => [d.day, d.count])), [overview]);
  const filtered = f.tag !== null || f.day !== null || f.id !== null || q !== "";
  const one = f.id !== null ? notes.find((n) => n.id === f.id) : undefined;
  usePageContext({
    module: "notes",
    filter: [f.tag ? `#${f.tag}` : null, f.day, q ? `搜「${q}」` : null].filter(Boolean).join(" · ") || null,
    record: one ? { kind: "note", id: one.id, label: `笔记：${one.content.replace(/\s+/g, " ").slice(0, 24)}` } : null,
  });

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

  useOpenRequest((h) => openNote(h.id));
  usePageKeys({
    onNew: () => editor.current?.focus(),
    onFind: () => searchRef.current?.focus(),
    onEscape: toTodo.selection !== null ? () => void toTodo.request(null) : f.id !== null ? () => setF(NONE) : undefined,
  });

  const stat = (n: number | undefined, label: string) => (
    <div className="flex flex-col">
      <span className="text-xl font-bold tabular-nums">{n ?? "–"}</span>
      <span className="text-xs text-muted-foreground">{label}</span>
    </div>
  );

  const nav = (
    <nav aria-label={mod.label} className="-m-1 flex w-[248px] shrink-0 flex-col overflow-y-auto p-1 pr-2">
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
      {tagNames.length > 0 && <TagTree tags={overview?.tags ?? []} pinned={overview?.pinned ?? []} active={f.tag} onSelect={(tag) => go({ tag })} />}
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
        <section ref={scroller} className="-m-1 min-w-0 flex-1 overflow-y-auto p-1">
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
                onToTodo={(x) => void toTodo.request(x.id)}
              />
            ))}
            {shown < notes.length && <div ref={sentinel} className="h-8" />}
            {notesQ.data && notes.length === 0 && !(near.length > 0 || similar.isFetching) && (
              <p className="py-12 text-center text-muted-foreground">{overview?.total === 0 && !filtered ? "念念不忘，必有回响" : "没有找到"}</p>
            )}
            {nearShown && shown >= notes.length && (
              <>
                <p className="flex items-center gap-1.5 pt-3 text-sm text-muted-foreground">
                  <Sparkles size={14} strokeWidth={1.75} />
                  意思相近
                  {similar.isFetching && near.length === 0 && <span className="animate-pulse">· 正在找…</span>}
                  {similar.isError && <span className="truncate">· {errorText(similar.error)}</span>}
                </p>
                {near.map((n) => (
                  <NoteCard key={`near-${n.id}`} note={n} tags={tagNames} focused={false} onTag={(tag) => go({ tag })} onOpen={openNote} onQuote={quote} onToTodo={(x) => void toTodo.request(x.id)} />
                ))}
              </>
            )}
          </div>
        </section>
      </div>
      {converting && (
        <TaskPanel
          key={converting.id}
          task={null}
          create={{ where: groups[0] ? `g:${groups[0].id}` : "", title: todoTitle(converting.content), note: converting.content.trim(), fromNote: converting.id }}
          parentTitle={null}
          hasChildren={false}
          groups={groups}
          goals={goals}
          onClose={() => void toTodo.request(null)}
          onDone={() => {
            toTodo.done();
            void qc.invalidateQueries({ queryKey: NOTES_KEY });
          }}
          onDirtyChange={toTodo.setDirty}
        />
      )}
      {toTodo.confirmDialog}
    </div>
  );
}
