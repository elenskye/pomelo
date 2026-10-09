// Task rows: a nested tree for one place (group / goal), or a flat list (今天 / 回看 / 月历).
// A row shows only the title and the due date (a repeat task's current occurrence). Circle = the
// type's quick action (Rust decides what it means): check, finish this occurrence, check in today,
// or +1. Row click = edit; drag the row = reorder among siblings; 「1/3 ⌄」 on the right = fold.
import { useState, type ReactNode } from "react";
import { CheckCircle2, ChevronDown, ChevronRight, Circle, Link2, ListPlus, X } from "lucide-react";
import { slashDate } from "@/shared/dates";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { InlineMd } from "@/shared/InlineMd";
import { api } from "@/shared/ipc";
import { useSortable } from "@/shared/sortable";
import { IconButton } from "@/shared/ui";
import { buildTree, useTodoMutations, type PlacedTask, type Task, type TreeNode } from "./api";

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
      message: subtasks === null ? "它下面如果有子任务，会一起删除。" : subtasks > 0 ? `它下面的 ${subtasks} 个子任务会一起删除。` : undefined,
      confirmLabel: "删除",
      danger: true,
    });
    if (ok) m.deleteTask.mutate(t.id, { onSuccess: () => toast("已删除") });
  };
  return [run, confirmDialog] as const;
}

const countSub = (n: TreeNode): number => n.children.reduce((k, c) => k + 1 + countSub(c), 0);

type RowProps = {
  task: Task;
  today: string;
  depth?: number;
  fold?: { open: boolean; done: number; total: number; onToggle: () => void };
  selected?: boolean;
  drag?: ReturnType<ReturnType<typeof useSortable>["handle"]>;
  onOpen: (t: Task) => void;
  onAddSub?: (t: Task) => void;
  onDelete: () => void;
};

function TaskRow(p: RowProps) {
  const m = useTodoMutations();
  const toast = useToast();
  const t = p.task;
  const struck = t.done && t.taskType !== "checkin";
  const late = !t.done && t.dueDate !== null && t.dueDate < p.today;
  return (
    <div
      {...p.drag}
      onClick={() => p.onOpen(t)}
      style={{ paddingLeft: 12 + (p.depth ?? 0) * 24 }}
      className={`group flex min-h-10 cursor-pointer items-center gap-2.5 border-b py-1.5 pr-2 transition-colors duration-150 ${
        p.selected ? "bg-accent" : "bg-card hover:bg-muted/70"
      }`}
    >
      <ActionCircle task={t} onToggle={() => m.setDone.mutate({ id: t.id, done: !(t.done || (t.taskType === "checkin" && t.loggedToday)) })} />
      <div className="min-w-0 flex-1">
        <span className={struck ? "text-muted-foreground line-through decoration-1" : undefined}>
          <InlineMd text={t.title} />
        </span>
        {t.dueDate && (
          <span className={`ml-2 text-xs whitespace-nowrap tabular-nums ${late ? "text-destructive" : "text-muted-foreground"}`}>{slashDate(t.dueDate)}</span>
        )}
      </div>
      <div onClick={(e) => e.stopPropagation()} className="flex items-center">
        {t.link && (
          <IconButton label="打开链接" onClick={() => void api.openLink(t.link!).catch((e) => toast(errorText(e), "error"))}>
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
        {p.fold && (
          <button
            type="button"
            aria-label={p.fold.open ? "收起子任务" : "展开子任务"}
            aria-expanded={p.fold.open}
            onClick={p.fold.onToggle}
            className="ml-1 flex items-center gap-0.5 rounded-control px-1.5 py-0.5 text-xs text-muted-foreground tabular-nums outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          >
            {p.fold.done}/{p.fold.total}
            <ChevronDown size={13} className={`transition-transform duration-150 ${p.fold.open ? "" : "-rotate-90"}`} />
          </button>
        )}
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
              drag={props.nodes.length > 1 ? sort.handle(t.id) : undefined}
              fold={
                hasKids
                  ? {
                      open: !t.collapsed,
                      done: n.doneLeaves,
                      total: n.leaves,
                      onToggle: () => m.quickUpdateTask.mutate({ id: t.id, patch: { collapsed: !t.collapsed } }),
                    }
                  : undefined
              }
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

/** A place's tasks as a tree. Undone top-level tasks first; done ones fold into 「已完成」 at the bottom
 * (with `doneGoals` — a group's finished goals — on top of them). */
export function TaskTree(props: TreeProps & { tasks: Task[]; empty?: ReactNode; doneGoals?: { count: number; node: ReactNode } }) {
  const [del, delDialog] = useDeleteTask();
  const [showDone, setShowDone] = useState(false);
  const roots = buildTree(props.tasks);
  const open = roots.filter((n) => !n.task.done);
  const done = roots.filter((n) => n.task.done);
  const doneGoalCount = props.doneGoals?.count ?? 0;
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
      {done.length + doneGoalCount > 0 && (
        <section>
          <button
            type="button"
            aria-expanded={showDone}
            onClick={() => setShowDone((v) => !v)}
            className="mb-1.5 flex items-center gap-1 px-1 text-sm text-muted-foreground hover:text-foreground"
          >
            {showDone ? <ChevronDown size={14} /> : <ChevronRight size={14} />} 已完成 {done.length + doneGoalCount}
          </button>
          {showDone && (
            <div className="space-y-2">
              {doneGoalCount > 0 && props.doneGoals?.node}
              {done.length > 0 && (
                <div className="overflow-hidden rounded-card border bg-card">
                  <Siblings {...props} nodes={done} depth={0} onDelete={onDelete} />
                </div>
              )}
            </div>
          )}
        </section>
      )}
      {delDialog}
    </div>
  );
}

/** Flat rows from any place (今天 / 回看 / 月历); same look as the tree, no nesting or dragging. */
export function PlacedList(props: { items: PlacedTask[]; today: string; selectedId: number | null; onOpen: (t: Task) => void }) {
  const [del, delDialog] = useDeleteTask();
  return (
    <div className="overflow-hidden rounded-card border bg-card">
      {props.items.map((p, i) => (
        <TaskRow
          key={`${p.id}-${i}`}
          task={p}
          today={props.today}
          selected={p.id === props.selectedId}
          onOpen={props.onOpen}
          onDelete={() => void del(p, null)}
        />
      ))}
      {delDialog}
    </div>
  );
}
