import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { currentMonth, MonthInput } from "@/shared/MonthInput";
import { SidePanel } from "@/shared/panel";
import { STATUS_ORDER, type StatusCode } from "@/shared/status";
import { Field, Segmented, TextArea, TextInput } from "@/shared/ui";
import { DRAWING_STATUS, useDrawingMutations, type Lesson, type LessonPatch } from "./api";

type Draft = {
  title: string;
  code: string;
  kindId: number | null;
  status: StatusCode;
  finishedMonth: string | null;
  note: string;
};

const toDraft = (l: Lesson | null, defaultKindId: number | null): Draft => ({
  title: l?.title ?? "",
  code: l?.code ?? "",
  kindId: l ? l.kindId : defaultKindId,
  status: l?.status ?? "unmarked",
  finishedMonth: l?.finishedMonth ?? null,
  note: l?.note ?? "",
});

// Full form → every key explicit (Rust won't auto-fill the month behind the user's back).
const toPatch = (d: Draft): LessonPatch => ({
  title: d.title,
  code: d.code.trim() || null,
  kindId: d.kindId,
  status: d.status,
  finishedMonth: d.finishedMonth,
  note: d.note.trim() || null,
});

/** Lesson editor. `lesson === null` = create (appended to `courseId`). */
export function LessonPanel(props: {
  courseId: number;
  lesson: Lesson | null;
  onClose: () => void;
  onDone: () => void;
  onDirtyChange: (dirty: boolean) => void;
  onManageKinds: () => void;
}) {
  const { lesson } = props;
  const isNew = lesson === null;
  const { data: kinds = [] } = useKinds("drawing");
  const defaultKindId = kinds[0]?.id ?? null;
  const initial = useMemo(() => toDraft(lesson, defaultKindId), [lesson, defaultKindId]);
  const [d, setD] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const m = useDrawingMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();

  useEffect(() => {
    if (isNew) setD((x) => (x.kindId === null ? { ...x, kindId: defaultKindId } : x));
  }, [isNew, defaultKindId]);

  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps

  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));
  const setStatus = (status: StatusCode) =>
    setD((x) => ({
      ...x,
      status,
      finishedMonth: status === "done" && x.status !== "done" && !x.finishedMonth ? currentMonth() : x.finishedMonth,
    }));

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      if (isNew) await m.createLesson.mutateAsync({ courseId: props.courseId, patch: toPatch(d) });
      else if (dirty) await m.updateLesson.mutateAsync({ id: lesson.id, patch: toPatch(d) });
      toast(isNew ? `已添加「${d.title.trim()}」` : "已保存");
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };

  const del = async () => {
    if (!lesson) return;
    const ok = await confirm({
      title: `删除「${lesson.title}」？`,
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      await m.deleteLesson.mutateAsync(lesson.id);
      toast(`已删除「${lesson.title}」`);
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <SidePanel
      title={isNew ? "新课节" : "编辑课节"}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={lesson ? () => void del() : undefined}
      error={error}
      meta={isNew ? "新课节会加在这门课的最后。" : `添加于 ${new Date(lesson.createdAt).toLocaleDateString("zh-CN")}`}
      extra={confirmDialog}
    >
      <Field label="课名">
        <TextInput autoFocus value={d.title} onChange={(e) => set("title", e.target.value)} placeholder="必填" />
      </Field>
      <Field label="编号">
        <TextInput value={d.code} onChange={(e) => set("code", e.target.value)} maxLength={20} />
      </Field>
      <Field label="类型">
        <div>
          <Dropdown
            ariaLabel="类型"
            value={d.kindId}
            onChange={(v) => set("kindId", v)}
            options={[...kinds.map((k) => ({ value: k.id as number | null, label: k.name })), { value: null, label: UNCATEGORIZED }]}
            actions={[{ label: "管理类型…", onSelect: props.onManageKinds }]}
          />
        </div>
      </Field>
      <Field label="状态">
        <Segmented
          ariaLabel="状态"
          value={d.status}
          options={STATUS_ORDER.map((s) => ({ value: s, label: DRAWING_STATUS[s] }))}
          onChange={setStatus}
        />
      </Field>
      <Field label="完成时间">
        <MonthInput value={d.finishedMonth} onChange={(v) => set("finishedMonth", v)} />
      </Field>
      <Field label="备注">
        <TextArea rows={5} value={d.note} onChange={(e) => set("note", e.target.value)} />
      </Field>
    </SidePanel>
  );
}
