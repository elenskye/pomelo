// 管理 (bottom of the conversation list): 模板 · 用途预设 · 全局指令 · 记忆 (Memory.tsx). Template
// edits save when a field loses focus (or on switching / closing); presets use a small form; the global
// instructions save with 保存. Every save is one undoable step in Rust.
import { useEffect, useRef, useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Plus, Trash2, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { Button, IconButton, Segmented, Switch, TextArea, TextInput } from "@/shared/ui";
import { INSTRUCTIONS_KEY, PRESETS_KEY, TEMPLATES_KEY, pickyApi, usePresets, useTemplates, type Preset, type ProjectRow, type Template } from "./api";
import { MemoriesTab } from "./Memory";
import { slots, templateGroups } from "./slots";

export type ManageTab = "templates" | "presets" | "global" | "memories";

export function ManageDialog(props: { tab: ManageTab; projects: ProjectRow[]; sessionIds: number[]; onOpenSession: (id: number) => void; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [tab, setTab] = useState<ManageTab>(props.tab);
  const [confirm, confirmDialog] = useConfirm();
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  return (
    <dialog
      ref={ref}
      aria-label="管理"
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className="m-auto flex h-[78vh] max-h-[720px] w-[860px] max-w-[92vw] flex-col rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <div className="flex items-center gap-4 border-b px-5 py-3">
        <p className="text-base font-bold">管理</p>
        <Segmented<ManageTab>
          value={tab}
          onChange={setTab}
          options={[
            { value: "templates", label: "模板" },
            { value: "presets", label: "用途预设" },
            { value: "global", label: "全局指令" },
            { value: "memories", label: "记忆" },
          ]}
        />
        <div className="flex-1" />
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      <div className="flex min-h-0 flex-1">
        {tab === "templates" && <Templates confirm={confirm} />}
        {tab === "presets" && <Presets confirm={confirm} />}
        {tab === "global" && <Global />}
        {tab === "memories" && (
          <MemoriesTab
            projects={props.projects}
            sessionIds={props.sessionIds}
            confirm={confirm}
            onOpenSession={(id) => {
              props.onClose();
              props.onOpenSession(id);
            }}
          />
        )}
      </div>
      {confirmDialog}
    </dialog>
  );
}

type Confirm = ReturnType<typeof useConfirm>[0];

function Templates(props: { confirm: Confirm }) {
  const qc = useQueryClient();
  const toast = useToast();
  const list = useTemplates().data ?? [];
  const [selected, setSelected] = useState<number | null>(null);
  const current = list.find((t) => t.id === selected) ?? (selected == null ? list[0] : undefined);
  const refresh = () => Promise.all([qc.invalidateQueries({ queryKey: TEMPLATES_KEY })]);
  const fail = (e: unknown) => toast(errorText(e), "error");

  const create = async () => {
    try {
      const id = await pickyApi.templateSave(null, { title: "新模板", category: current?.category ?? "", body: "" });
      await refresh();
      setSelected(id);
    } catch (e) {
      fail(e);
    }
  };
  const remove = async (t: Template) => {
    if (!(await props.confirm({ title: `删除模板「${t.title}」？`, message: "已经发出的消息不受影响。", confirmLabel: "删除", danger: true }))) return;
    try {
      await pickyApi.templateDelete(t.id);
      setSelected(null);
      await refresh();
    } catch (e) {
      fail(e);
    }
  };

  return (
    <>
      <div className="flex w-56 shrink-0 flex-col border-r">
        <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1 px-3 py-3">
          {templateGroups(list).map((g) => (
            <div key={g.title} className="mb-2">
              <p className="px-2 pt-1 pb-0.5 text-xs text-muted-foreground">{g.title}</p>
              {g.items.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  onClick={() => setSelected(t.id)}
                  className={`block w-full truncate rounded-control px-2 py-1.5 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring ${current?.id === t.id ? "bg-accent" : "hover:bg-muted/70"}`}
                >
                  {t.title}
                </button>
              ))}
            </div>
          ))}
        </div>
        <div className="border-t p-2">
          <button type="button" onClick={() => void create()} className="flex items-center gap-1.5 rounded-control px-2 py-1.5 text-sm text-muted-foreground hover:bg-muted hover:text-foreground">
            <Plus size={14} /> 新建模板
          </button>
        </div>
      </div>
      <div className="min-w-0 flex-1">{current && <TemplateEditor key={current.id} template={current} categories={[...new Set(list.map((t) => t.category).filter(Boolean))].sort()} onDelete={() => void remove(current)} />}</div>
    </>
  );
}

function TemplateEditor(props: { template: Template; categories: string[]; onDelete: () => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const t = props.template;
  const [title, setTitle] = useState(t.title);
  const [category, setCategory] = useState(t.category);
  const [body, setBody] = useState(t.body);
  const latest = useRef({ title, category, body });
  latest.current = { title, category, body };
  const saved = useRef({ title: t.title, category: t.category, body: t.body });

  const flush = async () => {
    const v = latest.current;
    const s = saved.current;
    if (v.title === s.title && v.category === s.category && v.body === s.body) return;
    saved.current = { ...v };
    try {
      await pickyApi.templateSave(t.id, v);
    } catch (e) {
      toast(errorText(e), "error");
    }
    void qc.invalidateQueries({ queryKey: TEMPLATES_KEY });
  };
  // switching templates or closing saves what was typed
  useEffect(() => () => void flush(), []); // eslint-disable-line react-hooks/exhaustive-deps

  const pin = (pinned: boolean) =>
    void pickyApi.templatePin(t.id, pinned).then(
      () => qc.invalidateQueries({ queryKey: TEMPLATES_KEY }),
      (e) => toast(errorText(e), "error"),
    );
  const names = [...new Set(slots(body).map((s) => s.name))];

  return (
    <div className="flex h-full flex-col gap-3 p-5">
      <div className="flex flex-col gap-1.5">
        <span className="text-xs text-muted-foreground">名称</span>
        <TextInput aria-label="名称" value={title} maxLength={60} onChange={(e) => setTitle(e.target.value)} onBlur={() => void flush()} />
      </div>
      <div className="flex items-end gap-3">
        <div className="flex flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">分类</span>
          <div className="flex items-center gap-1">
            <TextInput aria-label="分类" className="w-40" value={category} maxLength={20} onChange={(e) => setCategory(e.target.value)} onBlur={() => void flush()} />
            {props.categories.length > 0 && (
              <Dropdown<string | null>
                plain
                ariaLabel="选择分类"
                value={null}
                options={props.categories.map((c) => ({ value: c, label: c }))}
                onChange={(c) => {
                  if (c == null) return;
                  setCategory(c);
                  latest.current = { ...latest.current, category: c };
                  void flush();
                }}
                renderTrigger={() => <span>选择</span>}
                triggerClassName="shrink-0 rounded-control px-2 py-1.5 text-sm whitespace-nowrap text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
              />
            )}
          </div>
        </div>
        <div className="flex-1" />
        <div className="flex items-center gap-2 pb-2 text-sm">
          置顶 <Switch label="置顶" checked={t.pinned} onChange={pin} />
        </div>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-1.5">
        <span className="text-xs text-muted-foreground">内容</span>
        <TextArea aria-label="内容" value={body} onChange={(e) => setBody(e.target.value)} onBlur={() => void flush()} className="min-h-0 flex-1 resize-none font-reading leading-relaxed" />
      </div>
      <div className="flex items-center gap-3">
        {names.length > 0 && <p className="text-xs text-muted-foreground">待填位置：{names.join("、")}</p>}
        <div className="flex-1" />
        <Button variant="ghost" className="text-destructive" onClick={props.onDelete}>
          删除模板…
        </Button>
      </div>
    </div>
  );
}

function Presets(props: { confirm: Confirm }) {
  const qc = useQueryClient();
  const toast = useToast();
  const list = usePresets().data ?? [];
  const [editing, setEditing] = useState<Preset | "new" | null>(null);
  const refresh = () => Promise.all([qc.invalidateQueries({ queryKey: PRESETS_KEY }), qc.invalidateQueries({ queryKey: INSTRUCTIONS_KEY })]);
  const remove = async (p: Preset) => {
    if (!(await props.confirm({ title: `删除预设「${p.name}」？`, message: "用它的对话会改为不用预设。", confirmLabel: "删除", danger: true }))) return;
    void pickyApi.presetDelete(p.id).then(refresh, (e) => toast(errorText(e), "error"));
  };
  return (
    <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
      <div className="space-y-2 px-5 py-4">
        {list.map((p) => (
          <div key={p.id} className="flex items-start gap-3 rounded-card border bg-card px-4 py-3">
            <div className="min-w-0 flex-1">
              <p className="text-sm font-bold">{p.name}</p>
              <p className="mt-1 line-clamp-3 font-reading text-sm leading-relaxed whitespace-pre-wrap text-muted-foreground">{p.content}</p>
            </div>
            <IconButton label="编辑" onClick={() => setEditing(p)}>
              <Pencil size={14} />
            </IconButton>
            <IconButton label="删除预设" danger onClick={() => void remove(p)}>
              <Trash2 size={14} />
            </IconButton>
          </div>
        ))}
        <button type="button" onClick={() => setEditing("new")} className="flex items-center gap-1.5 rounded-control px-2 py-1.5 text-sm text-muted-foreground hover:bg-muted hover:text-foreground">
          <Plus size={14} /> 新建预设
        </button>
      </div>
      {editing && <PresetDialog preset={editing === "new" ? null : editing} onClose={() => setEditing(null)} onSaved={refresh} />}
    </div>
  );
}

function PresetDialog(props: { preset: Preset | null; onClose: () => void; onSaved: () => void }) {
  const [name, setName] = useState(props.preset?.name ?? "");
  const [content, setContent] = useState(props.preset?.content ?? "");
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await pickyApi.presetSave(props.preset?.id ?? null, name, content);
      props.onSaved();
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal title={props.preset ? "编辑用途预设" : "新建用途预设"} onClose={props.onClose} onSubmit={submit} error={error} submitLabel="保存" wide>
      <div className="flex flex-col gap-1.5">
        <span className="text-xs text-muted-foreground">名称</span>
        <TextInput autoFocus value={name} maxLength={30} onChange={(e) => setName(e.target.value)} />
      </div>
      <div className="flex flex-col gap-1.5">
        <span className="text-xs text-muted-foreground">内容</span>
        <TextArea rows={8} value={content} onChange={(e) => setContent(e.target.value)} className="font-reading" />
      </div>
    </Modal>
  );
}

function Global() {
  const qc = useQueryClient();
  const toast = useToast();
  const saved = useQuery({ queryKey: ["picky", "global-instructions"], queryFn: pickyApi.globalInstructions });
  const [text, setText] = useState<string | null>(null);
  const value = text ?? saved.data ?? "";
  const dirty = text != null && text.trim() !== (saved.data ?? "");
  const save = async () => {
    try {
      await pickyApi.globalInstructionsSave(value);
      await qc.invalidateQueries({ queryKey: ["picky", "global-instructions"] });
      void qc.invalidateQueries({ queryKey: INSTRUCTIONS_KEY });
      setText(null);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 px-5 py-4">
      <TextArea aria-label="全局指令" value={value} onChange={(e) => setText(e.target.value)} className="min-h-0 flex-1 resize-none font-reading leading-relaxed" />
      <div className="flex justify-end gap-2">
        {dirty && <Button onClick={() => setText(null)}>还原</Button>}
        <Button variant="primary" disabled={!dirty} onClick={() => void save()}>
          保存
        </Button>
      </div>
    </div>
  );
}
