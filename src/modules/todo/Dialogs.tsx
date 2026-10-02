// Modals for groups (name only) and goals (name, group, dates, status, note, 复盘).
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { Trash2 } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button, Field, inputClass, Segmented, TextArea, TextInput } from "@/shared/ui";
import { GOAL_STATUS, useTodoMutations, type Goal, type GoalStatus, type TodoGroup } from "./api";

function Modal(props: { title: string; onClose: () => void; onSubmit: (e: FormEvent) => void; error: string | null; children: ReactNode; footerLeft?: ReactNode; submitLabel: string; extra?: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  return (
    <dialog
      ref={ref}
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className="m-auto h-fit w-[440px] rounded-card border bg-popover p-5 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <form onSubmit={props.onSubmit} className="space-y-4">
        <p className="text-base font-bold">{props.title}</p>
        {props.children}
        {props.error && <p className="text-sm text-destructive">{props.error}</p>}
        <div className="flex items-center gap-2 pt-1">
          {props.footerLeft}
          <div className="flex-1" />
          <Button onClick={props.onClose}>取消</Button>
          <Button type="submit" variant="primary">
            {props.submitLabel}
          </Button>
        </div>
      </form>
      {props.extra}
    </dialog>
  );
}

export function GroupDialog(props: { group: TodoGroup | null; goalCount: number; onClose: () => void; onCreated?: (g: TodoGroup) => void; onDeleted?: () => void }) {
  const { group } = props;
  const [name, setName] = useState(group?.name ?? "");
  const [error, setError] = useState<string | null>(null);
  const m = useTodoMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      if (group) {
        await m.renameGroup.mutateAsync({ id: group.id, name });
        toast("已保存");
      } else {
        const g = await m.createGroup.mutateAsync(name);
        toast(`已添加分组「${g.name}」`);
        props.onCreated?.(g);
      }
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  const del = async () => {
    if (!group) return;
    const what = props.goalCount > 0 ? `里面的 ${props.goalCount} 个目标和所有任务` : "里面的所有任务";
    if (!(await confirm({ title: `删除分组「${group.name}」？`, message: `${what}会一起删除。`, confirmLabel: "删除", danger: true }))) return;
    try {
      await m.deleteGroup.mutateAsync(group.id);
      toast(`已删除分组「${group.name}」`);
      props.onDeleted?.();
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Modal
      title={group ? "编辑分组" : "新分组"}
      onClose={props.onClose}
      onSubmit={save}
      error={error}
      submitLabel={group ? "保存" : "新建"}
      extra={confirmDialog}
      footerLeft={
        group && (
          <Button variant="ghost-danger" onClick={() => void del()}>
            <Trash2 size={14} /> 删除分组
          </Button>
        )
      }
    >
      <Field label="分组名">
        <TextInput autoFocus value={name} onChange={(e) => setName(e.target.value)} maxLength={30} />
      </Field>
    </Modal>
  );
}

export function GoalDialog(props: {
  goal: Goal | null; // null = create
  groupId: number;
  groups: TodoGroup[];
  presetStatus?: GoalStatus; // e.g. opened from 「标记完成」
  onClose: () => void;
  onCreated?: (g: Goal) => void;
  onDeleted?: () => void;
}) {
  const { goal } = props;
  const [name, setName] = useState(goal?.name ?? "");
  const [groupId, setGroupId] = useState(goal?.groupId ?? props.groupId);
  const [status, setStatus] = useState<GoalStatus>(props.presetStatus ?? goal?.status ?? "active");
  const [startDate, setStartDate] = useState(goal?.startDate ?? "");
  const [endDate, setEndDate] = useState(goal?.endDate ?? "");
  const [note, setNote] = useState(goal?.note ?? "");
  const [review, setReview] = useState(goal?.review ?? "");
  const [error, setError] = useState<string | null>(null);
  const m = useTodoMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const closed = status === "done" || status === "abandoned";

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const patch = { name, groupId, status, startDate: startDate || null, endDate: endDate || null, note: note.trim() || null, review: review.trim() || null };
    try {
      if (goal) {
        await m.updateGoal.mutateAsync({ id: goal.id, patch });
        toast("已保存");
      } else {
        const g = await m.createGoal.mutateAsync(patch);
        toast(`已添加目标「${g.name}」`);
        props.onCreated?.(g);
      }
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  const del = async () => {
    if (!goal) return;
    const msg = goal.total > 0 ? "目标里的任务会一起删除。" : undefined;
    if (!(await confirm({ title: `删除目标「${goal.name}」？`, message: msg, confirmLabel: "删除", danger: true }))) return;
    try {
      await m.deleteGoal.mutateAsync(goal.id);
      toast(`已删除目标「${goal.name}」`);
      props.onDeleted?.();
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Modal
      title={goal ? "编辑目标" : "新目标"}
      onClose={props.onClose}
      onSubmit={save}
      error={error}
      submitLabel={goal ? "保存" : "新建"}
      extra={confirmDialog}
      footerLeft={
        goal && (
          <Button variant="ghost-danger" onClick={() => void del()}>
            <Trash2 size={14} /> 删除目标
          </Button>
        )
      }
    >
      <Field label="目标">
        <TextInput autoFocus={!props.presetStatus} value={name} onChange={(e) => setName(e.target.value)} placeholder="必填" maxLength={100} />
      </Field>
      <Field label="分组">
        <div>
          <Dropdown ariaLabel="分组" value={groupId} onChange={setGroupId} options={props.groups.map((g) => ({ value: g.id, label: g.name }))} />
        </div>
      </Field>
      <Field label="时间">
        <div className="flex items-center gap-2">
          <input type="date" aria-label="开始日期" value={startDate} onChange={(e) => setStartDate(e.target.value)} className={inputClass} />
          <span className="text-muted-foreground">到</span>
          <input type="date" aria-label="结束日期" value={endDate} onChange={(e) => setEndDate(e.target.value)} className={inputClass} />
        </div>
      </Field>
      {goal && (
        <Field label="状态">
          <Segmented
            ariaLabel="状态"
            value={status}
            onChange={setStatus}
            options={(Object.keys(GOAL_STATUS) as GoalStatus[]).map((s) => ({ value: s, label: GOAL_STATUS[s] }))}
          />
        </Field>
      )}
      <Field label="备注">
        <TextArea rows={2} value={note} onChange={(e) => setNote(e.target.value)} maxLength={5000} />
      </Field>
      {closed && (
        <Field label="复盘">
          <TextArea autoFocus={!!props.presetStatus} rows={4} value={review} onChange={(e) => setReview(e.target.value)} maxLength={5000} />
        </Field>
      )}
    </Modal>
  );
}
