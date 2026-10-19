// Side panel for one 循序渐进 lesson (called what the module says): 名称 · 编号 · 类型 · 状态 · 完成时间 · 备注.
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { UNCATEGORIZED, type Kind } from "@/shared/kinds/api";
import { currentMonth, MonthInput } from "@/shared/MonthInput";
import { SidePanel } from "@/shared/panel";
import type { StatusCode } from "@/shared/status";
import { Tag } from "@/shared/tags";
import { Field, Segmented, TextArea, TextInput } from "@/shared/ui";
import { useCustomMutations, type Lesson, type LessonPatch } from "./api";

export const PROGRESS_ORDER: StatusCode[] = ["todo", "doing", "done"];
export const PROGRESS_STATUS = { unmarked: "未开始", todo: "未开始", doing: "进行中", done: "已完成" };

type Draft = { title: string; code: string; kindId: number | null; status: StatusCode; finishedMonth: string | null; note: string };

const toDraft = (l: Lesson | null): Draft => ({ title: l?.title ?? "", code: l?.code ?? "", kindId: l?.kindId ?? null, status: l?.status ?? "todo", finishedMonth: l?.finishedMonth ?? null, note: l?.note ?? "" });

// every key explicit (Rust won't fill the month behind the user's back)
const toPatch = (d: Draft): LessonPatch => ({ title: d.title, code: d.code.trim() || null, kindId: d.kindId, status: d.status, finishedMonth: d.finishedMonth, note: d.note.trim() || null });

export function LessonPanel(props: { courseId: number; word: string; lesson: Lesson | null; kinds: Kind[]; onManageKinds: () => void; onClose: () => void; onDone: () => void; onDirtyChange: (dirty: boolean) => void }) {
  const { lesson, word } = props;
  const initial = useMemo(() => toDraft(lesson), [lesson]);
  const [d, setD] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const m = useCustomMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));
  const setStatus = (status: StatusCode) => setD((x) => ({ ...x, status, finishedMonth: status === "done" && x.status !== "done" && !x.finishedMonth ? currentMonth() : x.finishedMonth }));

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      if (!lesson) await m.lessonCreate.mutateAsync({ courseId: props.courseId, patch: toPatch(d) });
      else if (dirty) await m.lessonUpdate.mutateAsync({ id: lesson.id, patch: toPatch(d) });
      toast(lesson ? "已保存" : `已添加「${d.title.trim()}」`);
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const del = async () => {
    if (!lesson || !(await confirm({ title: `删除「${lesson.title}」？`, confirmLabel: "删除", danger: true }))) return;
    m.lessonDelete.mutate(lesson.id, {
      onSuccess: () => {
        toast("已删除");
        props.onDone();
      },
    });
  };

  return (
    <SidePanel
      title={lesson ? `编辑${word}` : `新${word}`}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={lesson ? () => void del() : undefined}
      error={error}
      meta={lesson ? `添加于 ${new Date(lesson.createdAt).toLocaleDateString("zh-CN")}` : undefined}
      extra={confirmDialog}
    >
      <Field label="名称">
        <TextInput autoFocus value={d.title} onChange={(e) => set("title", e.target.value)} placeholder="必填" maxLength={200} />
      </Field>
      <Field label="编号">
        <TextInput value={d.code} onChange={(e) => set("code", e.target.value)} maxLength={20} />
      </Field>
      <Field label="类型">
        <Dropdown<number | null>
          ariaLabel="类型"
          value={d.kindId}
          onChange={(v) => set("kindId", v)}
          menuMaxHeight={280}
          options={[...props.kinds.map((k, i) => ({ value: k.id as number | null, label: <Tag index={i}>{k.name}</Tag> })), { value: null, label: UNCATEGORIZED }]}
          actions={[{ label: "管理类型…", onSelect: props.onManageKinds }]}
        />
      </Field>
      <Field label="状态">
        <Segmented ariaLabel="状态" value={d.status} options={PROGRESS_ORDER.map((s) => ({ value: s, label: PROGRESS_STATUS[s] }))} onChange={setStatus} />
      </Field>
      <Field label="完成时间">
        <MonthInput value={d.finishedMonth} onChange={(v) => set("finishedMonth", v)} />
      </Field>
      <Field label="备注">
        <TextArea rows={5} value={d.note} onChange={(e) => set("note", e.target.value)} maxLength={5000} />
      </Field>
    </SidePanel>
  );
}
