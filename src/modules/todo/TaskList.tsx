// Task rows: a nested tree for one place (group / goal), or a flat list with context (今天 / 回看).
// Circle = the type's quick action (Rust decides what it means): check, finish this occurrence,
// check in today, or +1. Row click = edit; chevron = fold; grip = drag among siblings.
import { useState, type ReactNode } from "react";
import { CheckCircle2, ChevronDown, ChevronRight, Circle, Link2, ListPlus, Repeat, StickyNote, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { dayLabel } from "@/shared/dates";
import { useConfirm, useToast } from "@/shared/feedback";
import { InlineMd } from "@/shared/InlineMd";
import { DragHandle, useSortable } from "@/shared/sortable";
import { IconButton } from "@/shared/ui";
import { buildTree, progressLabel, ruleLabel, useTodoMutations, type PlacedTask, type Task, type TreeNode } from "./api";

/** "计划今天" but "计划 9 月 27 日". */
const spaced = (label: string) => (/^\d/.test(label) ? ` ${label}` : label);

/** Date summary for a row; red when the date has passed and the task isn't done. */
export function TaskDates(props: { task: Task; today: string }) {
  const { task: t, today } = props;
  if (t.taskType === "checkin") return null;
  const late = (d: string) => !t.done && d < today;
  const parts: ReactNode[] = [];
  if (t.startDate && t.dueDate) {
    parts.push(
      <span key="r" className={late(t.dueDate) ? "text-destructive" : undefined}>
        {dayLabel(t.startDate, today)} – {dayLabel(t.dueDate, today)}
      </span>,
    );
  } else if (t.dueDate) {
    parts.push(
      <span key="d" className={late(t.dueDate) ? "text-destructive" : undefined}>
        {dayLabel(t.dueDate, today)}截止
      </span>,
    );
  }
  if (t.planDate) {
    parts.push(
      <span key="p" className={!t.dueDate && late(t.planDate) ? "text-destructive" : undefined}>
        计划{spaced(dayLabel(t.planDate, today))}
      </span>,
    );
  }
  if (parts.length === 0) return null;
  return <span className="flex shrink-0 gap-2 text-xs text-muted-foreground tabular-nums">{parts}</span>;
}

/** 「每周六 · 第 7 / 20 次」「每天 · 连续 5 天」「本周 2 / 5 份」; empty for normal tasks. */
export function typeMeta(t: Task): string {
  switch (t.taskType) {
    case "repeat":
      return t.done
        ? `${ruleLabel(t.repeatRule)} · 共 ${t.count} 次，已结束`
        : `${ruleLabel(t.repeatRule)} · 第 ${t.count + 1}${t.repeatTotal ? ` / ${t.repeatTotal}` : ""} 次`;
    case "checkin":
      return [ruleLabel(t.repeatRule), `连续 ${t.streak} 天`, t.repeatTotal && `${t.count} / ${t.repeatTotal} 天`].filter(Boolean).join(" · ");
    case "progress":
      return progressLabel(t);
    default:
      return "";
  }
}

function Pie(props: { ratio: number }) {
  const r = 7;
  const c = 2 * Math.PI * r;
  return (
    <svg width="18" height="18" viewBox="0 0 18 18" className="text-muted-foreground hover:text-foreground">
      <circle cx="9" cy="9" r={r} fill="none" stroke="currentColor" strokeWidth="1.75" />
      <circle
        cx="9"
        cy="9"
        r={r / 2}
        fill="none"
        stroke="var(--status-done-fg)"
        strokeWidth={r}
        strokeDasharray={`${(Math.min(1, props.ratio) * c) / 2} ${c}`}
        transform="rotate(-90 9 9)"
      />
    </svg>
  );
}

function ActionCircle(props: { task: Task; onToggle: () => void }) {
  const t = props.task;
  const checked = t.done || (t.taskType === "checkin" && t.loggedToday);
  const label = t.done
    ? `把「${t.title}」标为没做完`
    : { normal: `完成「${t.title}」`, repeat: `完成这一次「${t.title}」`, checkin: t.loggedToday ? `取消今天的打卡` : `今天打卡「${t.title}」`, progress: `「${t.title}」+1` }[t.taskType];
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={(e) => {
        e.stopPropagation();
        props.onToggle();
      }}
      className="shrink-0 rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      {checked ? (
        <CheckCircle2 size={18} strokeWidth={1.75} style={{ color: "var(--status-done-fg)" }} />
      ) : t.taskType === "progress" ? (
        <Pie ratio={t.target ? t.count / t.target : 0} />
      ) : (
        <Circle size={18} strokeWidth={1.75} className="text-muted-foreground hover:text-foreground" />
      )}
    </button>
  );
}

/** Confirmed delete: names what goes with it. */
function useDeleteTask() {
  const m = useTodoMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  /** subtasks: count, or null when unknown (flat lists). */
  const run = async (t: Task, subtasks: number | null) => {
    const ok = await confirm({
      title: `删除「${t.title.replace(/\*\*/g, "")}」？`,
      message: `${subtasks === null ? "它下面如果有子任务，会一起删除。" : subtasks > 0 ? `它下面的 ${subtasks} 个子任务会一起删除。` : ""}删错了可以按 ⌘Z 撤销。`,
      confirmLabel: "删除",
      danger: true,
    });
    if (ok) m.deleteTask.mutate(t.id, { onSuccess: () => toast("已删除（⌘Z 撤销）") });
  };
  return [run, confirmDialog] as const;
}

const countSub = (n: TreeNode): number => n.children.reduce((k, c) => k + 1 + countSub(c), 0);

type RowProps = {
  task: Task;
  today: string;
  depth?: number;
  context?: ReactNode; // small line under the title (今天 / 回看)
  fold?: { open: boolean; onToggle: () => void };
  progress?: { done: number; total: number };
  selected?: boolean;
  grip?: ReactNode;
  onOpen: (t: Task) => void;
  onAddSub?: (t: Task) => void;
  onDelete: () => void;
};

function TaskRow(p: RowProps) {
  const m = useTodoMutations();
  const t = p.task;
  const meta = typeMeta(t);
  const struck = t.done && t.taskType !== "checkin";
  return (
    <div
      onClick={() => p.onOpen(t)}
      style={{ paddingLeft: 4 + (p.depth ?? 0) * 22 }}
      className={`group flex min-h-10 cursor-pointer items-center gap-2 border-b py-1.5 pr-2 transition-colors duration-150 ${
        p.selected ? "bg-accent" : "bg-card hover:bg-muted/70"
      }`}
    >
      <span className="flex w-4 shrink-0 justify-center">{p.grip}</span>
      <span className="flex w-4 shrink-0 justify-center">
        {p.fold && (
          <button
            type="button"
            aria-label={p.fold.open ? "收起子任务" : "展开子任务"}
            onClick={(e) => {
              e.stopPropagation();
              p.fold!.onToggle();
            }}
            className="rounded text-muted-foreground hover:text-foreground"
          >
            {p.fold.open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          </button>
        )}
      </span>
      <ActionCircle task={t} onToggle={() => m.setDone.mutate({ id: t.id, done: !(t.done || (t.taskType === "checkin" && t.loggedToday)) })} />
      <div className="min-w-0 flex-1">
        <div className={`flex items-center gap-1.5 ${struck ? "text-muted-foreground line-through decoration-1" : ""}`}>
          <span className="min-w-0">
            <InlineMd text={t.title} />
          </span>
          {t.taskType === "repeat" && <Repeat size={12} className="shrink-0 text-muted-foreground" aria-label="重复任务" />}
          {t.note && <StickyNote size={12} className="shrink-0 text-muted-foreground" aria-label="有备注" />}
          {p.progress && (
            <span className="shrink-0 text-xs text-muted-foreground no-underline">
              {p.progress.done}/{p.progress.total}
            </span>
          )}
        </div>
        {(p.context || meta) && (
          <div className="truncate text-xs text-muted-foreground">{[meta, p.context].filter(Boolean).join(" · ")}</div>
        )}
      </div>
      <TaskDates task={t} today={p.today} />
      <div onClick={(e) => e.stopPropagation()} className="flex">
        {t.link && (
          <IconButton label="打开链接" onClick={() => void openUrl(t.link!)}>
            <Link2 size={14} />
          </IconButton>
        )}
        <div className="flex opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100">
          {p.onAddSub && t.taskType === "normal" && (
            <IconButton label="添加子任务" onClick={() => p.onAddSub!(t)}>
              <ListPlus size={14} />
            </IconButton>
          )}
          <IconButton label={`删除「${t.title}」`} danger onClick={p.onDelete}>
            <X size={14} />
          </IconButton>
        </div>
      </div>
    </div>
  );
}

type TreeProps = {
  today: string;
  selectedId: number | null;
  onOpen: (t: Task) => void;
  onAddSub: (t: Task) => void;
};

/** One list of siblings (each node carries its own subtree), draggable among themselves. */
function Siblings(props: TreeProps & { nodes: TreeNode[]; depth: number; onDelete: (n: TreeNode) => void }) {
  const m = useTodoMutations();
  const sort = useSortable((ids) => m.reorderTasks.mutate(ids));
  return (
    <div ref={sort.listRef}>
      {props.nodes.map((n) => {
        const t = n.task;
        const hasKids = n.children.length > 0;
        return (
          <div key={t.id} {...sort.item(t.id)}>
            <TaskRow
              task={t}
              today={props.today}
              depth={props.depth}
              selected={t.id === props.selectedId}
              grip={props.nodes.length > 1 && <DragHandle {...sort.handle(t.id)} />}
              fold={hasKids ? { open: !t.collapsed, onToggle: () => m.quickUpdateTask.mutate({ id: t.id, patch: { collapsed: !t.collapsed } }) } : undefined}
              progress={hasKids ? { done: n.doneLeaves, total: n.leaves } : undefined}
              onOpen={props.onOpen}
              onAddSub={props.onAddSub}
              onDelete={() => props.onDelete(n)}
            />
            {hasKids && !t.collapsed && <Siblings {...props} nodes={n.children} depth={props.depth + 1} />}
          </div>
        );
      })}
    </div>
  );
}

/** A place's tasks as a tree. Undone top-level tasks first; done ones fold into 「已完成」 at the bottom. */
export function TaskTree(props: TreeProps & { tasks: Task[]; empty?: ReactNode }) {
  const [del, delDialog] = useDeleteTask();
  const [showDone, setShowDone] = useState(false);
  const roots = buildTree(props.tasks);
  const open = roots.filter((n) => !n.task.done);
  const done = roots.filter((n) => n.task.done);
  const onDelete = (n: TreeNode) => void del(n.task, countSub(n));

  return (
    <div className="space-y-4">
      {open.length > 0 ? (
        <div className="overflow-hidden rounded-card border bg-card">
          <Siblings {...props} nodes={open} depth={0} onDelete={onDelete} />
        </div>
      ) : (
        roots.length === 0 && props.empty
      )}
      {done.length > 0 && (
        <section>
          <button
            type="button"
            onClick={() => setShowDone((v) => !v)}
            className="mb-1.5 flex items-center gap-1 px-1 text-sm text-muted-foreground hover:text-foreground"
          >
            {showDone ? <ChevronDown size={14} /> : <ChevronRight size={14} />} 已完成 {done.length}
          </button>
          {showDone && (
            <div className="overflow-hidden rounded-card border bg-card">
              <Siblings {...props} nodes={done} depth={0} onDelete={onDelete} />
            </div>
          )}
        </section>
      )}
      {delDialog}
    </div>
  );
}

/** Flat rows with where each task lives (今天 / 回看 / 月历). `detail` = what happened (第 3 次 / 打卡 / +2 本). */
export function PlacedList(props: {
  items: (PlacedTask & { detail?: string | null })[];
  today: string;
  selectedId: number | null;
  onOpen: (t: Task) => void;
}) {
  const [del, delDialog] = useDeleteTask();
  return (
    <div className="overflow-hidden rounded-card border bg-card">
      {props.items.map((p, i) => (
        <TaskRow
          key={`${p.id}-${i}`}
          task={p}
          today={props.today}
          selected={p.id === props.selectedId}
          context={[p.detail, p.groupName, p.goalName, p.parentTitle?.replace(/\*\*/g, "")].filter(Boolean).join(" · ")}
          onOpen={props.onOpen}
          onDelete={() => void del(p, null)}
        />
      ))}
      {delDialog}
    </div>
  );
}
