// One note in the stream: time · pin · menu (编辑 / 引用 / 置顶 / 删除), the text (long ones fold),
// the notes it quotes and the notes quoting it. Double-click edits in place.
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { ChevronDown, CornerDownRight, Ellipsis, Pin, Quote, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button } from "@/shared/ui";
import { noteTime, useNoteMutations, type Brief, type Note } from "./api";
import { Editor } from "./Editor";
import { NoteBody } from "./NoteBody";

const FOLD_PX = 320;

export function BriefRow(props: { brief: Brief; icon: ReactNode; onOpen: (id: number) => void; onRemove?: () => void }) {
  return (
    <div className="flex min-w-0 items-center gap-1.5 text-sm text-muted-foreground">
      <span className="shrink-0">{props.icon}</span>
      <button type="button" onClick={() => props.onOpen(props.brief.id)} className="min-w-0 truncate text-left hover:text-foreground">
        {props.brief.excerpt}
      </button>
      {props.onRemove && (
        <button type="button" aria-label="不再引用" onClick={props.onRemove} className="shrink-0 rounded p-0.5 hover:bg-muted hover:text-foreground">
          <X size={12} />
        </button>
      )}
    </div>
  );
}

/** Text that folds past FOLD_PX with a 展开 toggle. */
function Folding(props: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [tall, setTall] = useState(false);
  const [open, setOpen] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setTall(el.scrollHeight > FOLD_PX + 40));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const folded = tall && !open;
  return (
    <div>
      <div ref={ref} className="relative overflow-hidden" style={folded ? { maxHeight: FOLD_PX } : undefined}>
        {props.children}
        {folded && <div className="pointer-events-none absolute inset-x-0 bottom-0 h-12 bg-gradient-to-t from-card to-transparent" />}
      </div>
      {tall && (
        <button type="button" onClick={() => setOpen(!open)} className="mt-1 flex items-center gap-1 text-sm text-primary hover:underline">
          {open ? "收起" : "展开全文"}
          <ChevronDown size={13} className={open ? "rotate-180" : ""} />
        </button>
      )}
    </div>
  );
}

export function NoteCard(props: {
  note: Note;
  tags: string[];
  onTag: (tag: string) => void;
  onOpen: (id: number) => void;
  onQuote: (n: Note) => void;
  focused?: boolean;
}) {
  const n = props.note;
  const m = useNoteMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const [draft, setDraft] = useState<{ content: string; refs: Brief[] } | null>(null);
  const [showBack, setShowBack] = useState(!!props.focused);
  const editing = draft !== null;

  const startEdit = () => setDraft({ content: n.content, refs: n.refs });
  const ids = (bs: Brief[]) => bs.map((b) => b.id).join();
  const dirty = draft !== null && (draft.content !== n.content || ids(draft.refs) !== ids(n.refs));
  const cancel = async () => {
    if (dirty && !(await confirm({ title: "放弃没有保存的修改？", confirmLabel: "放弃修改", danger: true }))) return;
    setDraft(null);
  };
  const save = () => {
    if (!draft) return;
    if (!dirty) return setDraft(null);
    m.update.mutate(
      { id: n.id, patch: { content: draft.content, refs: draft.refs.map((r) => r.id) } },
      { onSuccess: () => setDraft(null), onError: (e) => toast(errorText(e), "error") },
    );
  };
  const remove = async () => {
    const links = n.backrefs.length;
    const ok = await confirm({
      title: "删除这条笔记？",
      message: links > 0 ? `有 ${links} 条笔记引用了它，引用会一起去掉。` : undefined,
      confirmLabel: "删除",
      danger: true,
    });
    if (ok) m.remove.mutate(n.id, { onSuccess: () => toast("已删除") });
  };
  const act = (a: string) => {
    if (a === "edit") startEdit();
    else if (a === "quote") props.onQuote(n);
    else if (a === "pin") m.update.mutate({ id: n.id, patch: { pinned: !n.pinned } }, { onError: (e) => toast(errorText(e), "error") });
    else if (a === "delete") void remove();
  };

  return (
    <article
      onDoubleClick={(e) => {
        if (!editing && !(e.target as HTMLElement).closest("button")) startEdit();
      }}
      className={`group rounded-card border bg-card px-4 py-3 ${props.focused ? "ring-2 ring-ring" : ""}`}
    >
      <header className="mb-1.5 flex h-6 items-center gap-1.5 text-xs text-muted-foreground">
        <time dateTime={n.createdAt} className="tabular-nums">
          {noteTime(n.createdAt)}
        </time>
        {n.pinned && <Pin size={12} className="text-primary" aria-label="已置顶" />}
        <div className="flex-1" />
        {!editing && (
          <Dropdown<string | null>
            plain
            ariaLabel="更多"
            value={null}
            onChange={(a) => a && act(a)}
            align="end"
            options={[
              { value: "edit", label: "编辑" },
              { value: "quote", label: "引用" },
              { value: "pin", label: n.pinned ? "取消置顶" : "置顶" },
              { value: "delete", label: <span className="text-destructive">删除</span> },
            ]}
            renderTrigger={() => <Ellipsis size={16} />}
            triggerClassName="rounded-control p-1 text-muted-foreground opacity-0 outline-none group-hover:opacity-100 hover:bg-muted hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring aria-expanded:opacity-100"
          />
        )}
      </header>

      {editing ? (
        <div className="space-y-2">
          <Editor
            value={draft.content}
            onChange={(content) => setDraft({ ...draft, content })}
            onSubmit={save}
            onCancel={() => void cancel()}
            tags={props.tags}
            autoFocus
            minRows={2}
          />
          {draft.refs.length > 0 && (
            <div className="space-y-0.5">
              {draft.refs.map((r) => (
                <BriefRow
                  key={r.id}
                  brief={r}
                  icon={<Quote size={12} />}
                  onOpen={props.onOpen}
                  onRemove={() => setDraft({ ...draft, refs: draft.refs.filter((x) => x.id !== r.id) })}
                />
              ))}
            </div>
          )}
          <div className="flex justify-end gap-2 pt-1">
            <Button onClick={() => void cancel()}>取消</Button>
            <Button variant="primary" disabled={!draft.content.trim() || m.update.isPending} onClick={save}>
              保存
            </Button>
          </div>
        </div>
      ) : (
        <Folding>
          <NoteBody content={n.content} onTag={props.onTag} />
        </Folding>
      )}

      {!editing && n.refs.length > 0 && (
        <div className="mt-2 space-y-0.5 border-l-2 pl-2.5">
          {n.refs.map((r) => (
            <BriefRow key={r.id} brief={r} icon={<Quote size={12} />} onOpen={props.onOpen} />
          ))}
        </div>
      )}
      {!editing && n.backrefs.length > 0 && (
        <div className="mt-2">
          <button type="button" onClick={() => setShowBack(!showBack)} className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground">
            {n.backrefs.length} 条笔记引用了它
            <ChevronDown size={12} className={`transition-transform duration-150 ${showBack ? "" : "-rotate-90"}`} />
          </button>
          {showBack && (
            <div className="mt-1 space-y-0.5">
              {n.backrefs.map((r) => (
                <BriefRow key={r.id} brief={r} icon={<CornerDownRight size={12} />} onOpen={props.onOpen} />
              ))}
            </div>
          )}
        </div>
      )}
      {confirmDialog}
    </article>
  );
}
