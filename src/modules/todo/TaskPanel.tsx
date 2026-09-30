// Side panel to create / edit a task. Root tasks can move between groups and goals;
// subtasks stay under their parent. Completion is toggled from the list, not here.
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { formatDay, localToday, toYmd } from "@/shared/dates";
import { errorText, useToast } from "@/shared/feedback";
import { SidePanel } from "@/shared/panel";
import { Field, IconButton, inputClass, Segmented, TextArea, TextInput } from "@/shared/ui";
import { useTodoMutations, type Goal, type Task, type TaskPatch, type TodoGroup } from "./api";

type Deadline = "none" | "due" | "range";
type Draft = { title: string; where: string; planDate: string; deadline: Deadline; startDate: string; dueDate: string; link: string; note: string };

/** Where a new task goes. `where` = "g:<groupId>" or "o:<goalId>". */
export type NewTask = { where: string; parentId?: number; parentTitle?: string; planDate?: string };

/** UTC timestamp → local "10 月 1 日". */
const localDay = (ts: string) => formatDay(toYmd(new Date(ts)));
const whereOf = (t: Task) => (t.goalId !== null ? `o:${t.goalId}` : `g:${t.groupId}`);
const placeOf = (where: string): Pick<TaskPatch, "groupId" | "goalId"> => {
  const id = Number(where.slice(2));
  return where.startsWith("o:") ? { goalId: id } : { groupId: id, goalId: null };
};

function DateInput(props: { label: string; value: string; onChange: (v: string) => void }) {
  return (
    <div className="flex items-center gap-1">
      <input type="date" aria-label={props.label} value={props.value} onChange={(e) => props.onChange(e.target.value)} className={inputClass} />
      {props.value && (
        <IconButton label={`清除${props.label}`} onClick={() => props.onChange("")}>
          <X size={14} />
        </IconButton>
      )}
    </div>
  );
}

export function TaskPanel(props: {
  task: Task | null; // null = create
  create?: NewTask;
  parentTitle: string | null;
  groups: TodoGroup[];
  goals: Goal[];
  onClose: () => void;
  onDone: () => void;
  onDirtyChange: (dirty: boolean) => void;
  onDelete?: () => void;
}) {
  const { task } = props;
  const initial = useMemo<Draft>(
    () => ({
      title: task?.title ?? "",
      where: task ? whereOf(task) : (props.create?.where ?? ""),
      planDate: task?.planDate ?? props.create?.planDate ?? "",
      deadline: task?.startDate ? "range" : task?.dueDate ? "due" : "none",
      startDate: task?.startDate ?? "",
      dueDate: task?.dueDate ?? "",
      link: task?.link ?? "",
      note: task?.note ?? "",
    }),
    [task, props.create],
  );
  const [d, setD] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const m = useTodoMutations();
  const toast = useToast();
  const isSub = task ? task.parentId !== null : props.create?.parentId !== undefined;

  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps

  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));

  const whereOptions = props.groups.flatMap((g) => [
    { value: `g:${g.id}`, label: g.name },
    ...props.goals.filter((o) => o.groupId === g.id).map((o) => ({ value: `o:${o.id}`, label: `　◎ ${o.name}` })),
  ]);

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const fields: TaskPatch = {
      title: d.title,
      note: d.note.trim() || null,
      link: d.link.trim() || null,
      planDate: d.planDate || null,
      startDate: d.deadline === "range" ? d.startDate || null : null,
      dueDate: d.deadline === "none" ? null : d.dueDate || null,
    };
    try {
      if (!task) {
        const c = props.create!;
        await m.createTask.mutateAsync({ ...fields, ...(c.parentId !== undefined ? { parentId: c.parentId } : placeOf(d.where)) });
        toast(`已添加「${d.title.trim().replace(/\*\*/g, "")}」`);
      } else {
        const move = !isSub && d.where !== initial.where ? placeOf(d.where) : {};
        if (dirty) await m.updateTask.mutateAsync({ id: task.id, patch: { ...fields, ...move } });
        toast("已保存");
      }
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };

  const today = localToday();
  return (
    <SidePanel
      title={task ? "编辑任务" : isSub ? "添加子任务" : "添加任务"}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={props.onDelete}
      error={error}
      meta={
        task
          ? [`添加于 ${localDay(task.createdAt)}`, task.done && (task.doneAt ? `完成于 ${localDay(task.doneAt)}` : "已完成")]
              .filter(Boolean)
              .join(" · ")
          : undefined
      }
    >
      <Field label="任务">
        <TextInput autoFocus value={d.title} onChange={(e) => set("title", e.target.value)} placeholder="必填" maxLength={500} />
      </Field>
      <Field label="位置">
        {isSub ? (
          <p className="text-sm text-muted-foreground">在「{(props.parentTitle ?? props.create?.parentTitle ?? "").replace(/\*\*/g, "")}」下面</p>
        ) : (
          <div>
            <Dropdown ariaLabel="位置" value={d.where} onChange={(v) => set("where", v)} options={whereOptions} placeholder="选择分组或目标" />
          </div>
        )}
      </Field>
      <Field label="计划哪天做">
        <div className="flex items-center gap-2">
          <DateInput label="计划日期" value={d.planDate} onChange={(v) => set("planDate", v)} />
          {d.planDate !== today && (
            <button type="button" className="shrink-0 text-xs text-primary hover:underline" onClick={() => set("planDate", today)}>
              今天
            </button>
          )}
        </div>
      </Field>
      <Field label="截止">
        <div className="space-y-2">
          <Segmented
            ariaLabel="截止"
            value={d.deadline}
            onChange={(v) => set("deadline", v)}
            options={[
              { value: "none", label: "不设" },
              { value: "due", label: "截止日期" },
              { value: "range", label: "时间段" },
            ]}
          />
          {d.deadline === "due" && <DateInput label="截止日期" value={d.dueDate} onChange={(v) => set("dueDate", v)} />}
          {d.deadline === "range" && (
            <div className="flex flex-wrap items-center gap-2">
              <DateInput label="开始日期" value={d.startDate} onChange={(v) => set("startDate", v)} />
              <span className="text-muted-foreground">到</span>
              <DateInput label="结束日期" value={d.dueDate} onChange={(v) => set("dueDate", v)} />
            </div>
          )}
        </div>
      </Field>
      <Field label="链接">
        <TextInput value={d.link} onChange={(e) => set("link", e.target.value)} placeholder="https://…" maxLength={1000} />
      </Field>
      <Field label="备注">
        <TextArea rows={5} value={d.note} onChange={(e) => set("note", e.target.value)} placeholder="用 **两个星号** 包起来就是加粗" maxLength={5000} />
      </Field>
    </SidePanel>
  );
}
