// Create / edit / delete a course (modal).
import { useEffect, useRef, useState, type FormEvent } from "react";
import { Trash2 } from "lucide-react";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button, Field, TextArea, TextInput } from "@/shared/ui";
import { useDrawingMutations, type Course } from "./api";

export function CourseDialog(props: {
  course: Course | null; // null = create
  onClose: () => void;
  onCreated: (c: Course) => void;
  onDeleted: () => void;
}) {
  const { course } = props;
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const [name, setName] = useState(course?.name ?? "");
  const [note, setNote] = useState(course?.note ?? "");
  const [error, setError] = useState<string | null>(null);
  const m = useDrawingMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const patch = { name, note: note.trim() || null };
    try {
      if (course) {
        await m.updateCourse.mutateAsync({ id: course.id, patch });
        toast("已保存");
      } else {
        const c = await m.createCourse.mutateAsync(patch);
        toast(`已添加课程「${c.name}」`);
        props.onCreated(c);
      }
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  const del = async () => {
    if (!course) return;
    const ok = await confirm({
      title: `删除课程「${course.name}」？`,
      message: `其中的 ${course.total} 节课也会一起删除。删除后无法撤销，只能从备份里找回。`,
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      await m.deleteCourse.mutateAsync(course.id);
      toast(`已删除课程「${course.name}」`);
      props.onDeleted();
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <dialog
      ref={ref}
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className="m-auto h-fit w-[420px] rounded-card border bg-popover p-5 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <form onSubmit={save} className="space-y-4">
        <p className="text-base font-bold">{course ? "编辑课程" : "新课程"}</p>
        <Field label="课程名">
          <TextInput autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="必填" />
        </Field>
        <Field label="备注">
          <TextArea rows={3} value={note} onChange={(e) => setNote(e.target.value)} />
        </Field>
        {error && <p className="text-sm text-destructive">{error}</p>}
        <div className="flex items-center gap-2 pt-1">
          {course && (
            <Button variant="ghost-danger" onClick={() => void del()}>
              <Trash2 size={14} /> 删除课程
            </Button>
          )}
          <div className="flex-1" />
          <Button onClick={props.onClose}>取消</Button>
          <Button type="submit" variant="primary">
            保存
          </Button>
        </div>
      </form>
      {confirmDialog}
    </dialog>
  );
}
