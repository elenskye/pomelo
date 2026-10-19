// 指令 (the input's bottom row): a popover showing the layers the next reply sends (全局 · 项目 · 会话 ·
// 预设, composed in Rust instructions.rs — the same text the request uses), the purpose preset picker and
// the conversation's own instructions. A draft keeps its choice on the page until its first question.
import { useEffect, useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { SlidersHorizontal } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { TextArea } from "@/shared/ui";
import { INSTRUCTIONS_KEY, pickyApi, usePresets, type RetrievalPlan, type SessionSetting } from "./api";
import { toolClass } from "./Composer";

export const NO_SETTING: SessionSetting = { content: "", presetId: null };

/** What the project files do for the next question (the 项目资料 part of the preview). */
export type FilesPreview = { selected: string[]; plan: RetrievalPlan | null; cloud: boolean; onManage: () => void };

function filesLines(f: FilesPreview) {
  const out = [f.selected.length ? `这条消息使用全文：${f.selected.join("、")}。` : "这条消息没有选用资料。"];
  const ready = f.plan?.readyNames.length ?? 0;
  if (ready) out.push(`发送时从 ${ready} 份已建立索引的资料中检索相关段落。`);
  else out.push(f.selected.length ? "没有其他已建立索引的资料，不会自动检索。" : "没有已建立索引的资料，不会自动检索。");
  if (f.plan?.pending) out.push(`${f.plan.pending} 份资料的索引未完成，暂不检索。`);
  if (f.cloud && (f.selected.length || ready)) out.push("当前是云端模型，资料内容会一并发送。");
  return out;
}

export function InstructionsButton(props: {
  sessionId: number | null;
  /** A draft's project. */
  projectId: number | null;
  draft: SessionSetting;
  onDraftChange: (s: SessionSetting) => void;
  busy: boolean;
  onManage: () => void;
  /** Conversations in a project. */
  files?: FilesPreview | null;
}) {
  const qc = useQueryClient();
  const toast = useToast();
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const btn = useRef<HTMLButtonElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; bottom: number } | null>(null);
  const presets = usePresets().data ?? [];
  const preview = useQuery({
    queryKey: [...INSTRUCTIONS_KEY, props.sessionId, props.sessionId == null ? props.projectId : null, props.sessionId == null ? props.draft : null],
    queryFn: () => pickyApi.instructions(props.sessionId, props.projectId, props.sessionId == null ? props.draft : null),
  });
  const v = preview.data;
  const setting = v?.session ?? NO_SETTING;
  const preset = presets.find((p) => p.id === setting.presetId);

  const save = async (s: SessionSetting) => {
    if (props.sessionId == null) return props.onDraftChange(s);
    try {
      await pickyApi.sessionInstructionsSave(props.sessionId, s);
      await qc.invalidateQueries({ queryKey: INSTRUCTIONS_KEY });
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  useLayoutEffect(() => {
    if (!open || !btn.current) return;
    const r = btn.current.getBoundingClientRect();
    setPos({ left: Math.max(8, Math.min(r.left, window.innerWidth - 400)), bottom: window.innerHeight - r.top + 6 });
  }, [open]);
  useEffect(() => {
    if (!open) return;
    const outside = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      // the preset picker's menu lives in a portal of its own
      if (!pop.current?.contains(t) && !btn.current?.contains(t) && !t.closest('[role="listbox"]')) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
        btn.current?.focus();
      }
    };
    document.addEventListener("mousedown", outside);
    window.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", outside);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  const hasSession = !!v?.layers.some((l) => l.kind === "session");
  return (
    <>
      <button ref={btn} type="button" aria-expanded={open} onClick={() => {
          if (!open) void preview.refetch();
          setOpen(!open);
        }} className={`${toolClass} ${open ? "bg-muted text-foreground" : ""}`}>
        <SlidersHorizontal size={14} strokeWidth={1.5} /> 指令{preset && <span className="max-w-24 truncate">· {preset.name}</span>}
      </button>
      {open &&
        createPortal(
          <div
            ref={pop}
            role="dialog"
            aria-label="本轮生效的指令"
            style={{ position: "fixed", left: pos?.left ?? 0, bottom: pos?.bottom ?? 0, visibility: pos ? "visible" : "hidden" }}
            className="z-40 w-[380px] animate-pop-in rounded-card border bg-popover p-4 text-sm text-foreground shadow-lg"
          >
            <p className="font-bold">本轮生效的指令</p>
            <div className="-m-1 mt-2 max-h-80 space-y-3 overflow-y-auto p-1">
              {v && v.layers.length === 0 && <p className="text-muted-foreground">没有指令</p>}
              {v?.layers.map((l) => (
                <div key={l.kind}>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">{l.label}</span>
                    <div className="flex-1" />
                    {l.kind === "session" && (
                      <button type="button" disabled={props.busy} onClick={() => (setOpen(false), setEditing(true))} className="rounded-control px-1.5 text-xs text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-40">
                        编辑
                      </button>
                    )}
                  </div>
                  <p className="selectable mt-0.5 font-reading leading-relaxed break-words whitespace-pre-wrap">{l.content}</p>
                </div>
              ))}
            </div>
            {props.files && (
              <div className="mt-3 border-t pt-3">
                <div className="flex items-center">
                  <span className="text-xs text-muted-foreground">项目资料</span>
                  <div className="flex-1" />
                  <button
                    type="button"
                    onClick={() => {
                      setOpen(false);
                      props.files?.onManage();
                    }}
                    className="rounded-control px-1.5 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
                  >
                    管理…
                  </button>
                </div>
                {filesLines(props.files).map((l) => (
                  <p key={l} className="mt-1">
                    {l}
                  </p>
                ))}
              </div>
            )}
            <div className="mt-3 flex items-center gap-2 border-t pt-3">
              <span className="text-xs text-muted-foreground">用途预设</span>
              <span className={props.busy ? "pointer-events-none opacity-40" : ""}>
                <Dropdown<number | null>
                  ariaLabel="用途预设"
                  value={setting.presetId}
                  options={[{ value: null, label: "不使用" }, ...presets.map((p) => ({ value: p.id, label: p.name }))]}
                  onChange={(id) => void save({ ...setting, presetId: id })}
                />
              </span>
              <div className="flex-1" />
              {!hasSession && (
                <button type="button" disabled={props.busy} onClick={() => (setOpen(false), setEditing(true))} className="rounded-control px-2 py-1 text-xs text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-40">
                  添加会话指令
                </button>
              )}
            </div>
            <div className="mt-3 border-t pt-2">
              <button
                type="button"
                onClick={() => {
                  setOpen(false);
                  props.onManage();
                }}
                className="rounded-control px-1 py-1 text-xs text-muted-foreground hover:text-foreground"
              >
                编辑全局指令与预设…
              </button>
            </div>
          </div>,
          document.body,
        )}
      {editing && (
        <SessionDialog
          content={setting.content}
          onClose={() => setEditing(false)}
          onSave={async (content) => {
            await save({ ...setting, content });
            setEditing(false);
          }}
        />
      )}
    </>
  );
}

function SessionDialog(props: { content: string; onClose: () => void; onSave: (content: string) => Promise<void> }) {
  const [text, setText] = useState(props.content);
  const submit = (e: FormEvent) => {
    e.preventDefault();
    void props.onSave(text);
  };
  return (
    <Modal title="会话指令" onClose={props.onClose} onSubmit={submit} error={null} submitLabel="保存" wide>
      <TextArea autoFocus rows={8} value={text} onChange={(e) => setText(e.target.value)} className="font-reading" aria-label="会话指令" />
    </Modal>
  );
}
