// 知识库 (v0.4.5 阶段 7; rules in Rust knowledge.rs): the manager (「知识库」 at the bottom of the conversation
// list: libraries on the left — new, rename, delete, drag to reorder — and the chosen one's folders and saved
// web pages on the right, with how their index stands and 「发给云端」), the composer's 知识库 button (kept per
// conversation like 思考; its menu picks the libraries: the conversation's own, else its project's, else all)
// and the project's choice (项目设置). A draft keeps its own switch and libraries until its first question.
import { useEffect, useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Check, ChevronDown, ChevronRight, Ellipsis, Folder, Globe, LibraryBig, Plus, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { useSortable } from "@/shared/sortable";
import { Button, IconButton, Switch, TextInput } from "@/shared/ui";
import { LIBRARY_KEY, pickyApi, useLibrary, type KbDraft, type KbLibrary, type KbPlan, type KbSource, type Library } from "./api";
import { toolClass } from "./Composer";
import { WebDialog } from "./Files";

export const KB_KEY = ["picky", "kb"];

/** Libraries with their sources (polled while the manager is open: builds move on in the background). */
export function useKnowledgeBase(poll = false) {
  return useQuery({ queryKey: KB_KEY, queryFn: pickyApi.kbOverview, refetchInterval: poll ? 1500 : false });
}

const home = (p: string) => p.replace(/^\/Users\/[^/]+/, "~");

function sessionOf(lib: Library | undefined, id: number | null) {
  if (!lib || id == null) return undefined;
  return [...lib.pinned, ...lib.projects.flatMap((p) => p.sessions), ...lib.scheduled, ...lib.recent, ...lib.archived].find((s) => s.id === id);
}

/** The switch and libraries of `current` (a draft's own under `draftKey`, in `draftProject`). */
export function useKnowledge(current: number | null, draftKey: string, draftProject: number | null) {
  const qc = useQueryClient();
  const toast = useToast();
  const lib = useLibrary().data;
  const [drafts, setDrafts] = useState<Record<string, KbDraft>>({});
  const row = sessionOf(lib, current);
  const projectId = current == null ? draftProject : (row?.projectId ?? null);
  const project = lib?.projects.find((p) => p.id === projectId);
  // a new conversation in a project that chose libraries starts searching them
  const draft = drafts[draftKey] ?? { on: !!project?.libraries.length, libraries: [] };
  const on = current == null ? draft.on : !!row?.knowledge;
  const own = current == null ? draft.libraries : (row?.libraries ?? []);
  const done = (p: Promise<unknown>) =>
    void p.then(
      () => qc.invalidateQueries({ queryKey: LIBRARY_KEY }),
      (e) => toast(errorText(e), "error"),
    );
  const toggle = () => (current == null ? setDrafts((d) => ({ ...d, [draftKey]: { ...draft, on: !on } })) : done(pickyApi.conversationKnowledge(current, !on)));
  /** [] = follow the project (or all); choosing turns the switch on. */
  const choose = (ids: number[]) => (current == null ? setDrafts((d) => ({ ...d, [draftKey]: { on: true, libraries: ids } })) : done(pickyApi.conversationLibraries(current, ids)));
  /** What a draft sends and plans with (null for a saved conversation: Rust reads its own). */
  const forSend: KbDraft | null = current == null ? { on, libraries: own } : null;
  /** After a draft was sent (its conversation keeps the choice). */
  const clear = (key: string) => setDrafts(({ [key]: _, ...rest }) => rest);
  return { on, own, project: project ?? null, toggle, choose, forSend, clear };
}

/** A popover above an anchor (closes on Esc and clicks outside). */
function usePopover(width: number) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; bottom: number } | null>(null);
  useLayoutEffect(() => {
    if (!open || !anchor.current) return;
    const r = anchor.current.getBoundingClientRect();
    setPos({ left: Math.max(8, Math.min(r.left, window.innerWidth - width - 8)), bottom: window.innerHeight - r.top + 6 });
  }, [open, width]);
  useEffect(() => {
    if (!open) return;
    const outside = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!pop.current?.contains(t) && !anchor.current?.contains(t)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", outside);
    window.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", outside);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);
  return { open, setOpen, anchor, pop, pos };
}

/** The composer's 知识库 button (pressed: this conversation searches) and its menu of libraries. */
export function KnowledgeButton(props: { kb: ReturnType<typeof useKnowledge>; plan: KbPlan | null; onManage: () => void }) {
  const { kb } = props;
  const libraries = useKnowledgeBase().data?.libraries ?? [];
  const p = usePopover(256);
  const none = libraries.length === 0;
  const following = kb.own.filter((id) => libraries.some((l) => l.id === id)).length === 0;
  const projectLibs = kb.project?.libraries.filter((id) => libraries.some((l) => l.id === id)) ?? [];
  const followLabel = projectLibs.length ? `跟着项目（${projectLibs.map((id) => libraries.find((l) => l.id === id)?.name).join("、")}）` : "全部";
  const pick = (id: number) => {
    const next = following ? [id] : kb.own.includes(id) ? kb.own.filter((x) => x !== id) : [...kb.own, id];
    kb.choose(next);
  };
  const pressed = kb.on && !none;
  const row = "flex w-full items-center gap-2 rounded-control px-2 py-1.5 text-left text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring";
  const check = (on: boolean) => <Check size={14} className={`shrink-0 text-primary ${on ? "" : "invisible"}`} />;
  return (
    <>
      <div ref={p.anchor} className="flex shrink-0 items-center">
        <button
          type="button"
          aria-pressed={pressed}
          onClick={() => (none ? p.setOpen(!p.open) : kb.toggle())}
          className={`${toolClass} rounded-r-none pr-1 ${pressed ? "bg-accent text-accent-foreground" : ""}`}
        >
          <LibraryBig size={14} strokeWidth={1.5} /> 知识库
        </button>
        <button
          type="button"
          aria-label="选择知识库"
          aria-expanded={p.open}
          onClick={() => p.setOpen(!p.open)}
          className={`${toolClass} rounded-l-none px-1 ${pressed ? "bg-accent text-accent-foreground" : ""} ${p.open ? "bg-muted text-foreground" : ""}`}
        >
          <ChevronDown size={12} />
        </button>
      </div>
      {p.open &&
        createPortal(
          <div
            ref={p.pop}
            role="dialog"
            aria-label="知识库"
            style={{ position: "fixed", left: p.pos?.left ?? 0, bottom: p.pos?.bottom ?? 0, visibility: p.pos ? "visible" : "hidden" }}
            className="z-40 w-64 animate-pop-in rounded-card border bg-popover p-1.5 text-foreground shadow-lg"
          >
            {none ? (
              <p className="px-2 py-1.5 text-sm text-muted-foreground">还没有知识库</p>
            ) : (
              <>
                <div className="-m-1 max-h-60 overflow-y-auto p-1">
                  <button type="button" role="menuitemradio" aria-checked={following} onClick={() => kb.choose([])} className={row}>
                    {check(following)} <span className="min-w-0 flex-1 truncate">{followLabel}</span>
                  </button>
                  {libraries.map((l) => (
                    <button key={l.id} type="button" role="menuitemcheckbox" aria-checked={!following && kb.own.includes(l.id)} onClick={() => pick(l.id)} className={row}>
                      {check(!following && kb.own.includes(l.id))} <span className="min-w-0 flex-1 truncate">{l.name}</span>
                    </button>
                  ))}
                </div>
                {kb.on && props.plan && props.plan.skipped.length > 0 && <p className="px-2 pt-1 text-xs text-muted-foreground">用云端模型时不查：{props.plan.skipped.join("、")}</p>}
              </>
            )}
            <div className="mx-1 my-1 border-t" />
            <button
              type="button"
              className={row}
              onClick={() => {
                p.setOpen(false);
                props.onManage();
              }}
            >
              <span className="w-3.5" /> 管理知识库…
            </button>
          </div>,
          document.body,
        )}
    </>
  );
}

/** 项目设置: the libraries its conversations search (none chosen = all). */
export function ProjectLibraries(props: { value: number[]; onChange: (ids: number[]) => void }) {
  const libraries = useKnowledgeBase().data?.libraries ?? [];
  if (!libraries.length) return null;
  const chosen = props.value.filter((id) => libraries.some((l) => l.id === id));
  const chip = (on: boolean) => `rounded-control border px-2.5 py-1 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "border-primary bg-accent text-accent-foreground" : "hover:bg-muted"}`;
  return (
    <div className="flex flex-col gap-1.5 text-sm">
      <span>知识库</span>
      <div className="flex flex-wrap gap-1.5">
        <button type="button" aria-pressed={!chosen.length} onClick={() => props.onChange([])} className={chip(!chosen.length)}>
          全部
        </button>
        {libraries.map((l) => {
          const on = chosen.includes(l.id);
          return (
            <button key={l.id} type="button" aria-pressed={on} onClick={() => props.onChange(on ? chosen.filter((x) => x !== l.id) : [...chosen, l.id])} className={chip(on)}>
              {l.name}
            </button>
          );
        })}
      </div>
    </div>
  );
}

// ---------- the manager ----------

export function KnowledgeDialog(props: { onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const qc = useQueryClient();
  const toast = useToast();
  const data = useKnowledgeBase(true).data;
  const libraries = data?.libraries ?? [];
  const [chosen, setChosen] = useState<number | null>(null);
  const [naming, setNaming] = useState<{ id: number | null; name: string } | null>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
    void pickyApi.kbScan().catch(() => {});
  }, []);
  const shown = libraries.find((l) => l.id === chosen) ?? libraries[0];
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: KB_KEY });
    void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
  };
  const sort = useSortable((ids) => void pickyApi.kbLibrariesReorder(ids).then(refresh, (e) => toast(errorText(e), "error")));
  return (
    <dialog
      ref={ref}
      aria-label="知识库"
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className="m-auto flex h-[72vh] max-h-[660px] w-[820px] max-w-[92vw] flex-col rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <div className="flex items-center gap-4 border-b px-5 py-3">
        <p className="text-base font-bold">知识库</p>
        <div className="flex-1" />
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <nav aria-label="库" className="-m-1 flex w-52 shrink-0 flex-col overflow-y-auto border-r p-1 px-3 py-3">
          <div className="flex items-center justify-between px-2 pb-1">
            <p className="text-xs text-muted-foreground">库</p>
            <IconButton label="新建库" onClick={() => setNaming({ id: null, name: "" })}>
              <Plus size={13} strokeWidth={1.75} />
            </IconButton>
          </div>
          <div ref={sort.listRef}>
            {libraries.map((l) => (
              <div key={l.id} {...sort.item(l.id)} className="group">
                <button
                  type="button"
                  {...sort.handle(l.id)}
                  onClick={() => setChosen(l.id)}
                  aria-current={l.id === shown?.id}
                  className={`flex w-full items-center gap-2 rounded-control px-2 py-1.5 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring ${l.id === shown?.id ? "bg-accent" : "hover:bg-muted/70"}`}
                >
                  <span className="min-w-0 flex-1 truncate">{l.name}</span>
                  {l.sources.some((s) => s.building) && <span className="size-1.5 shrink-0 animate-pulse rounded-full bg-primary" aria-label="正在建立索引" />}
                </button>
              </div>
            ))}
          </div>
          {data && !libraries.length && (
            <Button className="mx-2 mt-2" onClick={() => setNaming({ id: null, name: "" })}>
              <Plus size={14} /> 新建库
            </Button>
          )}
        </nav>
        <div className="-m-1 min-w-0 flex-1 overflow-y-auto p-1 px-6 py-5">
          {data?.error && <p className="mb-3 rounded-control bg-destructive/10 px-3 py-2 text-sm text-destructive">没能建立索引：{data.error}</p>}
          {shown ? <LibraryPane library={shown} onRename={() => setNaming({ id: shown.id, name: shown.name })} onChanged={refresh} /> : data && <p className="py-10 text-center text-sm text-muted-foreground">还没有知识库</p>}
        </div>
      </div>
      {naming && (
        <NameDialog
          initial={naming.name}
          title={naming.id == null ? "新建库" : "改名"}
          onSave={async (name) => {
            if (naming.id == null) setChosen(await pickyApi.kbLibraryCreate(name));
            else await pickyApi.kbLibraryRename(naming.id, name);
            refresh();
          }}
          onClose={() => setNaming(null)}
        />
      )}
    </dialog>
  );
}

function NameDialog(props: { title: string; initial: string; onSave: (name: string) => Promise<void>; onClose: () => void }) {
  const [name, setName] = useState(props.initial);
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await props.onSave(name);
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal title={props.title} onClose={props.onClose} onSubmit={submit} error={error} submitLabel="保存">
      <TextInput autoFocus aria-label="名称" value={name} maxLength={30} onChange={(e) => setName(e.target.value)} />
    </Modal>
  );
}

function LibraryPane(props: { library: KbLibrary; onRename: () => void; onChanged: () => void }) {
  const l = props.library;
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const [web, setWeb] = useState(false);
  const act = (p: Promise<unknown>) => void p.then(props.onChanged, (e) => toast(errorText(e), "error"));
  const addFolder = async () => {
    const picked = await openDialog({ directory: true, multiple: false, title: "添加文件夹" });
    if (typeof picked === "string") act(pickyApi.kbFolderAdd(l.id, picked));
  };
  const removeLibrary = async () => {
    if (await confirm({ title: `删除知识库「${l.name}」？`, message: "文件夹里的文件不受影响。", confirmLabel: "删除", danger: true })) act(pickyApi.kbLibraryDelete(l.id));
  };
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <p className="min-w-0 flex-1 truncate text-base font-bold">{l.name}</p>
        <Dropdown<string | null>
          plain
          ariaLabel="添加"
          value={null}
          align="end"
          options={[
            { value: "folder", label: "文件夹…" },
            { value: "web", label: "网页…" },
          ]}
          onChange={(v) => (v === "folder" ? void addFolder() : v === "web" ? setWeb(true) : null)}
          renderTrigger={() => (
            <span className="flex items-center gap-1">
              <Plus size={14} /> 添加
            </span>
          )}
        />
        <Dropdown<string | null>
          plain
          ariaLabel={`知识库操作：${l.name}`}
          value={null}
          align="end"
          options={[{ value: "rename", label: "改名…" }]}
          actions={[{ label: <span className="text-destructive">删除库…</span>, onSelect: () => void removeLibrary() }]}
          onChange={(v) => v === "rename" && props.onRename()}
          renderTrigger={() => <Ellipsis size={14} />}
          triggerClassName="rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        />
      </div>
      {!l.sources.length && <p className="py-8 text-center text-sm text-muted-foreground">还没有文件夹或网页</p>}
      <div className="flex flex-col">
        {l.sources.map((s) => (
          <SourceRow key={s.id} source={s} act={act} confirm={confirm} />
        ))}
      </div>
      {web && (
        <WebDialog
          onAdded={(f) => act(pickyApi.kbWebAdd(l.id, f.id))}
          onClose={() => setWeb(false)}
        />
      )}
      {confirmDialog}
    </div>
  );
}

/** How a source's index stands, in a few words. */
function status(s: KbSource): { text: string; bad?: boolean } {
  if (s.missing) return { text: s.kind === "web" ? "网页副本已删除" : "找不到这个文件夹", bad: true };
  if (s.building) return { text: `正在建立索引 ${s.ready + s.failed.length}/${s.files}` };
  if (s.kind === "folder" && s.files === 0 && s.images === 0 && !s.scans) return { text: "没有小拾能读的文件" };
  const parts = [s.kind === "folder" ? `${s.files} 个文件` : s.pending ? "还没建立索引" : "已建立索引"];
  if (s.images > 0) parts.push(`${s.images} 张图片`);
  if (s.scans > 0) parts.push(`${s.scans} 个扫描件`);
  if (s.kind === "folder" && s.pending > 0) parts.push(`${s.pending} 个待更新`);
  if (s.truncated) parts.push("文件太多，只读了前 2000 个");
  return { text: parts.join(" · ") };
}

function SourceRow(props: { source: KbSource; act: (p: Promise<unknown>) => void; confirm: ReturnType<typeof useConfirm>[0] }) {
  const s = props.source;
  const toast = useToast();
  const [showFailed, setShowFailed] = useState(false);
  const st = status(s);
  const Icon = s.kind === "web" ? Globe : Folder;
  const open = (path: string) => void pickyApi.kbOpen(s.id, path).catch((e) => toast(errorText(e), "error"));
  const remove = async () => {
    if (await props.confirm({ title: `从知识库移除「${s.name}」？`, message: s.kind === "folder" ? "文件夹里的文件不受影响。" : undefined, confirmLabel: "移除", danger: true })) props.act(pickyApi.kbSourceRemove(s.id));
  };
  return (
    <div className="border-b py-2.5 last:border-b-0">
      <div className="flex items-center gap-3">
        <Icon size={16} strokeWidth={1.5} className="shrink-0 text-muted-foreground" />
        <div className="min-w-0 flex-1">
          <button type="button" onClick={() => open("")} className="block max-w-full truncate text-left text-sm hover:underline">
            {s.name}
          </button>
          <p className="truncate text-xs text-muted-foreground">{s.kind === "folder" ? home(s.path ?? "") : (s.url ?? "")}</p>
          <p className={`text-xs ${st.bad ? "text-destructive" : "text-muted-foreground"}`}>
            {st.text}
            {s.failed.length > 0 && (
              <button type="button" aria-expanded={showFailed} onClick={() => setShowFailed(!showFailed)} className="ml-1 inline-flex items-center gap-0.5 text-destructive hover:underline">
                · {s.failed.length} 个读不了 <ChevronRight size={11} className={`transition-transform duration-150 ${showFailed ? "rotate-90" : ""}`} />
              </button>
            )}
          </p>
        </div>
        <label className="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
          发给云端 <Switch label={`${s.name} 发给云端`} checked={s.cloud} onChange={(v) => props.act(pickyApi.kbSourceCloud(s.id, v))} />
        </label>
        <Dropdown<string | null>
          plain
          ariaLabel={`操作：${s.name}`}
          value={null}
          align="end"
          options={[
            { value: "open", label: s.kind === "web" ? "打开网页" : "在访达中打开" },
            { value: "rebuild", label: "重新建立索引" },
          ]}
          actions={[{ label: <span className="text-destructive">移除…</span>, onSelect: () => void remove() }]}
          onChange={(v) => (v === "open" ? open("") : v === "rebuild" ? props.act(pickyApi.kbSourceRebuild(s.id)) : null)}
          renderTrigger={() => <Ellipsis size={14} />}
          triggerClassName="rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        />
      </div>
      {showFailed && (
        <ul className="mt-1.5 ml-7 space-y-0.5 text-xs">
          {s.failed.map((f) => (
            <li key={f.path} className="flex gap-2">
              <button type="button" onClick={() => open(f.path)} className="min-w-0 shrink truncate text-left hover:underline">
                {f.path}
              </button>
              <span className="shrink-0 text-muted-foreground">{f.error}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** The citation's 「打开文件」 for a 知识库 passage. */
export function openKnowledge(sourceId: number, path: string, toast: ReturnType<typeof useToast>) {
  void pickyApi.kbOpen(sourceId, path).catch((e) => toast(errorText(e), "error"));
}

