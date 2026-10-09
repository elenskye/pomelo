// Typed wrappers for Rust todo_* commands (src-tauri/src/modules/todo.rs) + React Query hooks.
// Rules (placement, completion cascade, 今天 / 已过期) live in Rust; the UI only renders.
import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";

export type TodoGroup = { id: number; name: string; sortOrder: number; open: number; createdAt: string; updatedAt: string };

export type GoalStatus = "active" | "paused" | "done";
export const GOAL_STATUS: Record<GoalStatus, string> = { active: "进行中", paused: "暂停", done: "已完成" };

export type Goal = {
  id: number;
  groupId: number;
  name: string;
  status: GoalStatus;
  startDate: string | null;
  endDate: string | null;
  note: string | null;
  review: string | null;
  sortOrder: number;
  total: number; // leaf tasks
  done: number;
  createdAt: string;
  updatedAt: string;
};

export type TaskType = "normal" | "repeat" | "checkin" | "progress";
export const TASK_TYPES: Record<TaskType, string> = { normal: "普通", repeat: "重复", checkin: "打卡", progress: "进度条" };

export type Task = {
  id: number;
  groupId: number;
  goalId: number | null;
  parentId: number | null;
  title: string;
  note: string | null;
  link: string | null;
  /** Repeat (完成后): series start. */
  planDate: string | null;
  startDate: string | null; // normal / check-in / progress: optional start of the 时间段 ending at dueDate
  /** Normal / check-in / progress: end of the 时间段. Repeat: the current occurrence. */
  dueDate: string | null;
  done: boolean;
  doneAt: string | null;
  collapsed: boolean;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
  taskType: TaskType;
  /** daily | weekly:1,3 (Mon=1) | monthly:1,15 | yearly:12-31 | yearly:03-01/3 (every 3 years) | every:3d | every:2w | after:30d · 2w · 6m · 1y */
  repeatRule: string | null;
  repeatTotal: number | null; // repeat (all but 完成后): occurrences
  repeatUntil: string | null; // unused since v11
  target: number | null; // progress
  unit: string | null;
  /** Derived: repeat = occurrences done; checkin = days checked; progress = amount so far. */
  count: number;
  /** Derived, repeat with a total: date of the last occurrence. */
  lastDate: string | null;
  totalAmount: number; // progress
  loggedToday: boolean;
  streak: number; // checkin
};

export type Log = { id: number; taskId: number; day: string; amount: number; createdAt: string };

/** A task with where it lives (今天 / 回看 lists). */
export type PlacedTask = Task & { groupName: string; goalName: string | null; parentTitle: string | null };
export type Agenda = { today: string; overdue: PlacedTask[]; due: PlacedTask[]; doneToday: PlacedTask[] };
/** Something finished on `day` (a checked task, or one log: 第 3 次 / 打卡 / +2 本). */
export type DoneItem = PlacedTask & { day: string; at: string; detail: string | null };
/** Month grid entry. what: due | range | next (later occurrence of a repeat task) | done. */
export type CalItem = PlacedTask & { day: string; what: "due" | "range" | "next" | "done"; detail: string | null };

/** Omitted key = unchanged; null = clear. Create places by parentId > goalId > groupId. */
export type TaskPatch = Partial<
  Pick<Task, "title" | "note" | "link" | "planDate" | "startDate" | "dueDate" | "collapsed" | "taskType" | "repeatRule" | "repeatTotal" | "repeatUntil" | "target" | "unit">
> & {
  groupId?: number;
  goalId?: number | null;
  parentId?: number;
  /** Repeat: occurrences done so far. */
  doneCount?: number;
};
export type GoalPatch = Partial<Pick<Goal, "groupId" | "name" | "status" | "startDate" | "endDate" | "note" | "review">>;

/** Which place a task list shows. */
export type Place = { groupId: number; goalId?: undefined } | { goalId: number; groupId?: undefined };

const api = {
  groups: () => invoke<TodoGroup[]>("todo_groups"),
  createGroup: (name: string) => invoke<TodoGroup>("todo_group_create", { name }),
  renameGroup: (v: { id: number; name: string }) => invoke<TodoGroup>("todo_group_rename", v),
  deleteGroup: (id: number) => invoke<void>("todo_group_delete", { id }),
  goals: () => invoke<Goal[]>("todo_goals"),
  createGoal: (patch: GoalPatch) => invoke<Goal>("todo_goal_create", { patch }),
  updateGoal: (v: { id: number; patch: GoalPatch }) => invoke<Goal>("todo_goal_update", v),
  deleteGoal: (id: number) => invoke<void>("todo_goal_delete", { id }),
  tasks: (p: Place) => invoke<Task[]>("todo_tasks", { groupId: p.groupId ?? null, goalId: p.goalId ?? null }),
  createTask: (patch: TaskPatch) => invoke<Task>("todo_task_create", { patch }),
  updateTask: (v: { id: number; patch: TaskPatch }) => invoke<Task>("todo_task_update", v),
  setDone: (v: { id: number; done: boolean }) => invoke<void>("todo_task_set_done", v),
  deleteTask: (id: number) => invoke<void>("todo_task_delete", { id }),
  agenda: (today: string) => invoke<Agenda>("todo_agenda", { today }),
  doneBetween: (from: string, to: string) => invoke<DoneItem[]>("todo_done_between", { from, to }),
  month: (from: string, to: string) => invoke<CalItem[]>("todo_month", { from, to }),
  skip: (id: number) => invoke<void>("todo_task_skip", { id }),
  logs: (taskId: number) => invoke<Log[]>("todo_logs", { taskId }),
  log: (v: { taskId: number; day?: string; amount?: number }) => invoke<void>("todo_log", v),
  deleteLog: (id: number) => invoke<void>("todo_log_delete", { id }),
  reorderGroups: (ids: number[]) => invoke<void>("todo_groups_reorder", { ids }),
  reorderGoals: (v: { groupId: number; ids: number[] }) => invoke<void>("todo_goals_reorder", v),
  reorderTasks: (ids: number[]) => invoke<void>("todo_tasks_reorder", { ids }),
};

export const TODO_KEY = ["todo"];
export const useGroups = () => useQuery({ queryKey: [...TODO_KEY, "groups"], queryFn: api.groups });
export const useGoals = () => useQuery({ queryKey: [...TODO_KEY, "goals"], queryFn: api.goals });
export const useTasks = (p: Place | null) =>
  useQuery({ queryKey: [...TODO_KEY, "tasks", p], queryFn: () => api.tasks(p!), enabled: p !== null });
export const useAgenda = (today: string) => useQuery({ queryKey: [...TODO_KEY, "agenda", today], queryFn: () => api.agenda(today) });
export const useDoneBetween = (from: string, to: string) =>
  useQuery({ queryKey: [...TODO_KEY, "done", from, to], queryFn: () => api.doneBetween(from, to) });
export const useMonth = (from: string, to: string) => useQuery({ queryKey: [...TODO_KEY, "month", from, to], queryFn: () => api.month(from, to) });
export const useLogs = (taskId: number | null) =>
  useQuery({ queryKey: [...TODO_KEY, "logs", taskId], queryFn: () => api.logs(taskId!), enabled: taskId !== null });

export function useTodoMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  // Any change can move counts, progress and 今天 → refresh every todo query.
  const onSuccess = () => void qc.invalidateQueries({ queryKey: TODO_KEY });
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    createGroup: useMutation({ mutationFn: api.createGroup, onSuccess }),
    renameGroup: useMutation({ mutationFn: api.renameGroup, onSuccess }),
    deleteGroup: useMutation({ mutationFn: api.deleteGroup, onSuccess }),
    createGoal: useMutation({ mutationFn: api.createGoal, onSuccess }),
    updateGoal: useMutation({ mutationFn: api.updateGoal, onSuccess }),
    deleteGoal: useMutation({ mutationFn: api.deleteGoal, onSuccess }),
    createTask: useMutation({ mutationFn: api.createTask, onSuccess }),
    updateTask: useMutation({ mutationFn: api.updateTask, onSuccess }),
    /** Fire-and-forget (circle clicks, collapse toggles, row deletes): errors surface as toasts. */
    quickUpdateTask: useMutation({ mutationFn: api.updateTask, onSuccess, onError }),
    setDone: useMutation({ mutationFn: api.setDone, onSuccess, onError }),
    deleteTask: useMutation({ mutationFn: api.deleteTask, onSuccess, onError }),
    skip: useMutation({ mutationFn: api.skip, onSuccess, onError }),
    log: useMutation({ mutationFn: api.log, onSuccess, onError }),
    deleteLog: useMutation({ mutationFn: api.deleteLog, onSuccess, onError }),
    reorderGroups: useMutation({ mutationFn: api.reorderGroups, onSuccess, onError }),
    reorderGoals: useMutation({ mutationFn: api.reorderGoals, onSuccess, onError }),
    reorderTasks: useMutation({ mutationFn: api.reorderTasks, onSuccess, onError }),
  };
}

export type TreeNode = { task: Task; children: TreeNode[]; leaves: number; doneLeaves: number };

/** Flat list (one place) → forest in sort order, with leaf progress per node. */
export function buildTree(tasks: Task[]): TreeNode[] {
  const byParent = new Map<number | null, Task[]>();
  for (const t of tasks) byParent.set(t.parentId, [...(byParent.get(t.parentId) ?? []), t]);
  const build = (parent: number | null): TreeNode[] =>
    (byParent.get(parent) ?? []).map((task) => {
      const children = build(task.id);
      const leaves = children.length ? children.reduce((n, c) => n + c.leaves, 0) : 1;
      const doneLeaves = children.length ? children.reduce((n, c) => n + c.doneLeaves, 0) : task.done ? 1 : 0;
      return { task, children, leaves, doneLeaves };
    });
  return build(null);
}

export const pct = (done: number, total: number) => (total > 0 ? Math.round((done / total) * 100) : 0);
