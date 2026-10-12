// Memories on the page (rules in Rust memory.rs): the 记忆 tab of 管理 (filter, add, edit, switch,
// local-only, delete, source and dates), the /记住 confirmation card, the input's 记忆 menu (this
// question / this conversation), the preview lines and the memories a reply used.
import { useEffect, useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { useQueryClient } from "@tanstack/react-query";
import { Brain, Check, ChevronRight, Ellipsis, Plus } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { toYmd } from "@/shared/dates";
import { errorText, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { Button, Field, Switch, TextArea } from "@/shared/ui";
import { MEMORIES_KEY, MEMORY_MAX, pickyApi, useMemories, type Memory, type MemoryInput, type MemoryPlan, type ProjectRow, type UsedMemory } from "./api";
import { toolClass } from "./Composer";

type Confirm = (o: { title: string; message?: string; confirmLabel?: string; danger?: boolean }) => Promise<boolean>;

const count = (t: string) => [...t.trim()].length;

function MemoryText(props: { value: string; onChange: (v: string) => void; autoFocus?: boolean }) {
  const n = count(props.value);
  return (
    <div className="flex flex-col gap-1">
      <TextArea autoFocus={props.autoFocus} rows={3} value={props.value} onChange={(e) => props.onChange(e.target.value)} className="font-reading" aria-label="记忆内容" />
      <span className={`self-end text-xs ${n > MEMORY_MAX ? "text-destructive" : "text-muted-foreground"}`}>
        {n}/{MEMORY_MAX}
      </span>
    </div>
  );
}

function ScopePicker(props: { projects: ProjectRow[]; value: number | null; onChange: (v: number | null) => void }) {
  return (
    <Dropdown<number | null>
      ariaLabel="保存到"
      value={props.value}
      options={[{ value: null, label: "全局（所有对话）" }, ...props.projects.map((p) => ({ value: p.id, label: `项目：${p.name}` }))]}
      onChange={props.onChange}
    />
  );
}

/** 新建 / 编辑记忆: text (up to 300), where it applies, local-only, enabled. */
export function MemoryEditor(props: { memory: Memory | null; projects: ProjectRow[]; defaultProject?: number | null; onClose: () => void }) {
  const qc = useQueryClient();
  const m = props.memory;
  const [v, setV] = useState<MemoryInput>({
    content: m?.content ?? "",
    projectId: m ? m.projectId : (props.defaultProject ?? null),
    sharedProjectIds: m?.sharedProjectIds ?? [],
    enabled: m?.enabled ?? true,
    localOnly: m?.localOnly ?? false,
  });
  const [error, setError] = useState<string | null>(null);
  const valid = count(v.content) > 0 && count(v.content) <= MEMORY_MAX;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!valid) return;
    try {
      await pickyApi.memorySave(m?.id ?? null, { ...v, sharedProjectIds: v.projectId == null ? [] : v.sharedProjectIds });
      void qc.invalidateQueries({ queryKey: MEMORIES_KEY });
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const others = props.projects.filter((p) => p.id !== v.projectId);
  return (
    <Modal title={m ? "编辑记忆" : "新建记忆"} onClose={props.onClose} onSubmit={submit} error={error} submitLabel="保存" wide>
      <MemoryText autoFocus value={v.content} onChange={(content) => setV({ ...v, content })} />
      <Field label="保存到">
        <ScopePicker projects={props.projects} value={v.projectId} onChange={(projectId) => setV({ ...v, projectId })} />
      </Field>
      {v.projectId != null && others.length > 0 && (
        <Field label="同时用于">
          <div className="flex flex-wrap gap-x-4 gap-y-1.5">
            {others.map((p) => {
              const on = v.sharedProjectIds.includes(p.id);
              return (
                <button
                  key={p.id}
                  type="button"
                  role="checkbox"
                  aria-checked={on}
                  onClick={() => setV({ ...v, sharedProjectIds: on ? v.sharedProjectIds.filter((x) => x !== p.id) : [...v.sharedProjectIds, p.id] })}
                  className="flex items-center gap-1.5 text-sm"
                >
                  <span className={`flex size-4 items-center justify-center rounded border ${on ? "border-primary bg-primary text-primary-foreground" : ""}`}>{on && <Check size={11} />}</span>
                  {p.name}
                </button>
              );
            })}
          </div>
        </Field>
      )}
      <div className="flex items-center gap-6 text-sm">
        <span className="flex items-center gap-2">
          仅本地 <Switch label="仅本地" checked={v.localOnly} onChange={(localOnly) => setV({ ...v, localOnly })} />
        </span>
        <span className="flex items-center gap-2">
          启用 <Switch label="启用" checked={v.enabled} onChange={(enabled) => setV({ ...v, enabled })} />
        </span>
      </div>
    </Modal>
  );
}

/** /记住: check the entries, choose where they go, then save (the rest of the draft is sent after). */
export function RememberDialog(props: { entries: string[]; projects: ProjectRow[]; projectId: number | null; onSave: (entries: string[], projectId: number | null, localOnly: boolean) => Promise<void>; onClose: () => void }) {
  const [entries, setEntries] = useState(props.entries);
  const [projectId, setProjectId] = useState(props.projectId);
  const [localOnly, setLocalOnly] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const valid = entries.length > 0 && entries.every((e) => count(e) > 0 && count(e) <= MEMORY_MAX);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!valid) return;
    try {
      await props.onSave(
        entries.map((x) => x.trim()),
        projectId,
        localOnly,
      );
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal title={entries.length > 1 ? `记住这 ${entries.length} 条` : "记住"} onClose={props.onClose} onSubmit={submit} error={error} submitLabel="保存" wide>
      {entries.map((e, i) => (
        <MemoryText key={i} autoFocus={i === 0} value={e} onChange={(t) => setEntries(entries.map((x, j) => (j === i ? t : x)))} />
      ))}
      <Field label="保存到">
        <ScopePicker projects={props.projects} value={projectId} onChange={setProjectId} />
      </Field>
      <span className="flex items-center gap-2 text-sm">
        仅本地 <Switch label="仅本地" checked={localOnly} onChange={setLocalOnly} />
      </span>
    </Modal>
  );
}

function day(iso: string) {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso.slice(0, 10) : toYmd(d);
}

/** 记于 always; 添加 / 修改 when they differ. */
function dates(m: Memory) {
  const out = [`记于 ${m.notedOn}`];
  const added = day(m.createdAt);
  if (added !== m.notedOn) out.push(`添加 ${added}`);
  if (m.updatedAt !== m.createdAt) {
    const changed = day(m.updatedAt);
    out.push(changed !== added ? `修改 ${changed}` : "已修改");
  }
  return out.join(" · ");
}

function Tag(props: { children: string }) {
  return <span className="rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{props.children}</span>;
}

/** The 记忆 tab of 管理. */
export function MemoriesTab(props: { projects: ProjectRow[]; sessionIds: number[]; confirm: Confirm; onOpenSession: (id: number) => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const list = useMemories().data ?? [];
  const [filter, setFilter] = useState<string>("all");
  const [editing, setEditing] = useState<Memory | "new" | null>(null);
  const shown = list.filter((m) => (filter === "all" ? true : filter === "global" ? m.projectId == null : m.projectId === Number(filter) || m.sharedProjectIds.includes(Number(filter))));
  const act = (p: Promise<unknown>) => void p.then(() => qc.invalidateQueries({ queryKey: MEMORIES_KEY }), (e) => toast(errorText(e), "error"));
  const name = (id: number) => props.projects.find((p) => p.id === id)?.name;
  const scope = (m: Memory) => {
    if (m.projectId == null) return "全局";
    const shared = m.sharedProjectIds.map(name).filter(Boolean);
    return `项目：${name(m.projectId) ?? "已删除的项目"}${shared.length ? `，同时用于 ${shared.join("、")}` : ""}`;
  };
  const sourceText = (m: Memory) =>
    m.source === "import" ? "从用途预设拆分" : m.source === "manual" ? "手动添加" : m.sourceSessionId == null || props.sessionIds.includes(m.sourceSessionId) ? "对话中保存" : "对话中保存（对话已删除）";
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-3 px-5 pt-4 pb-2">
        <Dropdown<string>
          ariaLabel="范围"
          value={filter}
          options={[{ value: "all", label: "全部" }, { value: "global", label: "全局" }, ...props.projects.map((p) => ({ value: String(p.id), label: `项目：${p.name}` }))]}
          onChange={setFilter}
        />
        <span className="text-xs text-muted-foreground">{shown.length} 条</span>
        <div className="flex-1" />
        <Button onClick={() => setEditing("new")}>
          <Plus size={14} /> 新建记忆
        </Button>
      </div>
      <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
        <div className="space-y-2 px-5 pb-4">
          {!shown.length && <p className="py-6 text-center text-sm text-muted-foreground">{list.length ? "这个范围里没有记忆" : "还没有记忆"}</p>}
          {shown.map((m) => (
            <div key={m.id} className="flex items-start gap-3 rounded-card border bg-card px-4 py-3">
              <div className="min-w-0 flex-1">
                <p className={`selectable font-reading text-sm leading-relaxed whitespace-pre-wrap ${m.enabled ? "" : "text-muted-foreground"}`}>{m.content}</p>
                <div className="mt-1.5 flex flex-wrap gap-1.5">
                  <Tag>{scope(m)}</Tag>
                  {m.localOnly && <Tag>仅本地</Tag>}
                  {!m.enabled && <Tag>已停用</Tag>}
                </div>
                <p className="mt-1 text-xs text-muted-foreground">
                  {sourceText(m)}
                  {m.source === "command" && m.sourceSessionId != null && props.sessionIds.includes(m.sourceSessionId) && (
                    <button type="button" onClick={() => props.onOpenSession(m.sourceSessionId!)} className="ml-1 text-primary hover:underline">
                      打开对话
                    </button>
                  )}
                  {" · "}
                  {dates(m)}
                </p>
              </div>
              <Switch label={m.enabled ? "停用这条记忆" : "启用这条记忆"} checked={m.enabled} onChange={(on) => act(pickyApi.memoryFlag(m.id, "enabled", on))} />
              <Dropdown<string | null>
                plain
                ariaLabel="记忆操作"
                value={null}
                align="end"
                options={[
                  { value: "edit", label: "编辑…" },
                  { value: "local", label: m.localOnly ? "允许云端模型使用" : "设为仅本地" },
                ]}
                actions={[
                  {
                    label: <span className="text-destructive">删除…</span>,
                    onSelect: () =>
                      void props.confirm({ title: "删除这条记忆？", message: "用过它的回复仍会显示当时的内容。", confirmLabel: "删除", danger: true }).then((ok) => ok && act(pickyApi.memoryDelete(m.id))),
                  },
                ]}
                onChange={(v) => (v === "edit" ? setEditing(m) : v === "local" ? act(pickyApi.memoryFlag(m.id, "localOnly", !m.localOnly)) : null)}
                renderTrigger={() => <Ellipsis size={14} />}
                triggerClassName="rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
              />
            </div>
          ))}
        </div>
      </div>
      {editing && (
        <MemoryEditor
          memory={editing === "new" ? null : editing}
          projects={props.projects}
          defaultProject={filter !== "all" && filter !== "global" ? Number(filter) : null}
          onClose={() => setEditing(null)}
        />
      )}
    </div>
  );
}

/** The preview's 记忆 lines. */
export function memoryLines(p: MemoryPlan) {
  if (p.off === "everywhere") return ["设置里已停用记忆。"];
  if (p.off === "conversation") return ["这个对话已关闭记忆。"];
  if (p.off === "nextMessage") return ["下一条消息不用记忆。"];
  const out: string[] = [];
  if (!p.count) out.push("没有可用的记忆。");
  else {
    const parts = [p.global && `全局 ${p.global}`, p.project && `本项目 ${p.project}`, p.shared && `其他项目共享 ${p.shared}`].filter(Boolean);
    out.push(`发送时按问题从 ${p.count} 条记忆（${parts.join("、")}）中挑选相关的几条，附上记录日期。`);
  }
  if (p.skippedLocalOnly) out.push(`当前是云端模型，${p.skippedLocalOnly} 条仅本地记忆不会使用。`);
  return out;
}

/** The input's 记忆 button: this question / this conversation, and 管理记忆…. */
export function MemoryButton(props: { plan: MemoryPlan | null; nextOn: boolean; conversationOff: boolean; onNext: (on: boolean) => void; onConversation: (off: boolean) => void; onManage: () => void }) {
  const [open, setOpen] = useState(false);
  const btn = useRef<HTMLButtonElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; bottom: number } | null>(null);
  useLayoutEffect(() => {
    if (!open || !btn.current) return;
    const r = btn.current.getBoundingClientRect();
    setPos({ left: Math.max(8, Math.min(r.left, window.innerWidth - 250)), bottom: window.innerHeight - r.top + 6 });
  }, [open]);
  useEffect(() => {
    if (!open) return;
    const outside = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!pop.current?.contains(t) && !btn.current?.contains(t)) setOpen(false);
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
  const off = props.plan?.off ?? null;
  const label = off === "everywhere" ? "记忆已停用" : off === "conversation" ? "记忆已关闭" : off === "nextMessage" ? "本条不用记忆" : "记忆";
  const row = "flex w-full items-center gap-2 rounded-control px-2 py-1.5 text-left text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40";
  const check = (on: boolean) => <Check size={14} className={`shrink-0 text-primary ${on ? "" : "invisible"}`} />;
  return (
    <>
      <button ref={btn} type="button" aria-expanded={open} onClick={() => setOpen(!open)} className={`${toolClass} ${open ? "bg-muted text-foreground" : ""} ${off ? "opacity-60" : ""}`}>
        <Brain size={14} strokeWidth={1.5} /> {label}
      </button>
      {open &&
        createPortal(
          <div
            ref={pop}
            role="dialog"
            aria-label="记忆"
            style={{ position: "fixed", left: pos?.left ?? 0, bottom: pos?.bottom ?? 0, visibility: pos ? "visible" : "hidden" }}
            className="z-40 w-60 animate-pop-in rounded-card border bg-popover p-1.5 text-foreground shadow-lg"
          >
            <button type="button" role="menuitemcheckbox" aria-checked={props.nextOn} disabled={off === "everywhere" || off === "conversation"} onClick={() => props.onNext(!props.nextOn)} className={row}>
              {check(props.nextOn)} 本条使用记忆
            </button>
            <button type="button" role="menuitemcheckbox" aria-checked={props.conversationOff} disabled={off === "everywhere"} onClick={() => props.onConversation(!props.conversationOff)} className={row}>
              {check(props.conversationOff)} 本会话关闭记忆
            </button>
            <div className="mx-1 my-1 border-t" />
            <button
              type="button"
              className={row}
              onClick={() => {
                setOpen(false);
                props.onManage();
              }}
            >
              <span className="w-3.5" /> 管理记忆…
            </button>
          </div>,
          document.body,
        )}
    </>
  );
}

/** Under a reply: the memories its run was given (folded). */
export function UsedMemories(props: { memories: UsedMemory[] }) {
  if (!props.memories.length) return null;
  return (
    <details className="picky-think group/mem mt-1.5" data-find-skip>
      <summary className="inline-flex cursor-default list-none items-center gap-1 text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
        <ChevronRight size={12} className="transition-transform duration-150 group-open/mem:rotate-90" />
        本轮使用了 {props.memories.length} 条记忆
      </summary>
      <ul className="mt-1 space-y-1 border-l-2 pl-3 font-reading text-[13.5px] text-muted-foreground">
        {props.memories.map((m, i) => (
          <li key={i}>
            <span className="mr-1.5 text-xs">记于 {m.notedOn}</span>
            {m.content}
          </li>
        ))}
      </ul>
    </details>
  );
}
