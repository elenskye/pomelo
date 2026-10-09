// 一步一步: left nav (今天 · 月历 · 回看 · groups → goals; drag rows to reorder, fold a group's goals)
// + right content. Modeled on GoalMap. Goals carry no icon (names can start with an emoji).
import { useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { CalendarCheck, CalendarDays, ChevronDown, FolderPlus, ListPlus, Plus, Sun, Target } from "lucide-react";
import { moduleById } from "@/app/modules";
import { Dropdown } from "@/shared/Dropdown";
import { formatDay, localToday, weekday } from "@/shared/dates";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { InlineMd } from "@/shared/InlineMd";
import { isNumberArray, useSetting } from "@/shared/settings";
import { useOpenRequest } from "@/shared/open";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { useSortable } from "@/shared/sortable";
import { Button, EmptyState, PageHeader, ProgressBar } from "@/shared/ui";
import {
  GOAL_STATUS,
  pct,
  useAgenda,
  useGoals,
  useGroups,
  useTasks,
  useTodoMutations,
  type Goal,
  type GoalStatus,
  type PlacedTask,
  type Task,
  type TodoGroup,
} from "./api";
import { GoalDialog, GroupDialog } from "./Dialogs";
import { Section, ViewHeader } from "./layout";
import { MonthView } from "./MonthView";
import { ReviewView } from "./ReviewView";
import { PlacedList, TaskTree } from "./TaskList";
import { TaskPanel, type NewTask } from "./TaskPanel";

type View = { kind: "today" } | { kind: "month" } | { kind: "review" } | { kind: "group"; id: number } | { kind: "goal"; id: number };

/** Local date that follows the clock (re-checked when the window regains focus). */
function useToday() {
  const [today, setToday] = useState(localToday());
  useEffect(() => {
    const check = () => setToday(localToday());
    window.addEventListener("focus", check);
    return () => window.removeEventListener("focus", check);
  }, []);
  return today;
}

type DragProps = ReturnType<ReturnType<typeof useSortable>["handle"]>;

function NavButton(props: { active: boolean; onClick: () => void; children: ReactNode; indent?: boolean; drag?: DragProps; trailing?: ReactNode }) {
  return (
    <div {...props.drag} className={`flex items-center rounded-control transition-colors duration-150 ${props.active ? "bg-accent" : "hover:bg-muted/70"}`}>
      <button
        type="button"
        aria-current={props.active}
        onClick={props.onClick}
        className={`flex min-w-0 flex-1 items-center gap-2 rounded-control py-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring ${
          props.indent ? "pr-2.5 pl-6" : "px-2.5"
        }`}
      >
        {props.children}
      </button>
      {props.trailing}
    </div>
  );
}

const NAV_COLLAPSED = "todo.nav_collapsed";

/** Group ids whose goals are folded in the nav (kept in settings, outside undo). */
function useFoldedGroups() {
  const [ids, setIds] = useSetting<number[]>(NAV_COLLAPSED, [], isNumberArray);
  const toggle = (id: number) => setIds(ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id]);
  return [ids, toggle] as const;
}

/** Groups (and each group's goals) in the nav; drag a row to reorder, fold a group's goals on the right. */
function GroupNav(props: { groups: TodoGroup[]; goals: Goal[]; view: View; go: (v: View) => void }) {
  const m = useTodoMutations();
  const sort = useSortable((ids) => m.reorderGroups.mutate(ids));
  const [folded, toggle] = useFoldedGroups();
  return (
    <div ref={sort.listRef}>
      {props.groups.map((g) => {
        const goals = props.goals.filter((o) => o.groupId === g.id);
        const open = !folded.includes(g.id);
        return (
          <div key={g.id} {...sort.item(g.id)} className="bg-background">
            <NavButton
              active={props.view.kind === "group" && props.view.id === g.id}
              onClick={() => props.go({ kind: "group", id: g.id })}
              drag={props.groups.length > 1 ? sort.handle(g.id) : undefined}
              trailing={
                goals.length > 0 && (
                  <button
                    type="button"
                    aria-label={open ? `收起「${g.name}」的目标` : `展开「${g.name}」的目标`}
                    aria-expanded={open}
                    onClick={() => toggle(g.id)}
                    className="mr-1.5 rounded p-0.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    <ChevronDown size={13} className={`transition-transform duration-150 ${open ? "" : "-rotate-90"}`} />
                  </button>
                )
              }
            >
              <span className="min-w-0 flex-1 truncate">{g.name}</span>
              <span className="shrink-0 text-xs text-muted-foreground">{g.open || ""}</span>
            </NavButton>
            {open && <GoalNav goals={goals} groupId={g.id} view={props.view} go={props.go} />}
          </div>
        );
      })}
    </div>
  );
}

function GoalNav(props: { goals: Goal[]; groupId: number; view: View; go: (v: View) => void }) {
  const m = useTodoMutations();
  const sort = useSortable((ids) => m.reorderGoals.mutate({ groupId: props.groupId, ids }));
  return (
    <div ref={sort.listRef}>
      {props.goals.map((o) => (
        <div key={o.id} {...sort.item(o.id)} className="bg-background">
          <NavButton
            indent
            active={props.view.kind === "goal" && props.view.id === o.id}
            onClick={() => props.go({ kind: "goal", id: o.id })}
            drag={props.goals.length > 1 ? sort.handle(o.id) : undefined}
          >
            <GoalNavLabel goal={o} />
          </NavButton>
        </div>
      ))}
    </div>
  );
}

const closedGoal = (s: GoalStatus) => s === "done";

function GoalNavLabel({ goal }: { goal: Goal }) {
  const closed = closedGoal(goal.status);
  return (
    <>
      <span className={`min-w-0 flex-1 truncate text-sm ${closed ? "text-muted-foreground" : ""}`}>
        {goal.name}
      </span>
      <span className="shrink-0 text-xs text-muted-foreground">
        {goal.status === "active" ? `${pct(goal.done, goal.total)}%` : GOAL_STATUS[goal.status]}
      </span>
    </>
  );
}

function QuickAdd(props: { inputRef: RefObject<HTMLInputElement | null>; placeholder: string; onAdd: (title: string) => Promise<unknown>; extra?: ReactNode }) {
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const toast = useToast();
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        if (!title.trim() || busy) return;
        setBusy(true);
        try {
          await props.onAdd(title);
          setTitle("");
        } catch (err) {
          toast(errorText(err), "error");
        } finally {
          setBusy(false);
        }
      }}
      className="flex items-center gap-2 rounded-card border border-dashed bg-card/60 px-3 py-1.5 focus-within:border-solid focus-within:ring-2 focus-within:ring-ring"
    >
      <Plus size={16} className="shrink-0 text-muted-foreground" />
      <input
        ref={props.inputRef}
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        placeholder={props.placeholder}
        maxLength={500}
        className="min-w-0 flex-1 bg-transparent py-0.5 outline-none placeholder:text-muted-foreground"
      />
      {props.extra}
    </form>
  );
}

export default function TodoPage() {
  const m = moduleById("todo");
  const today = useToday();
  const { data: groups = [], isPending: groupsPending } = useGroups();
  const { data: goals = [], isPending: goalsPending } = useGoals();
  const agendaQ = useAgenda(today);
  const agenda = agendaQ.data;
  const [view, setView] = useState<View>({ kind: "today" });
  const [todayGroupId, setTodayGroupId] = useState<number | null>(null);
  const [groupDialog, setGroupDialog] = useState<{ group: TodoGroup | null } | null>(null);
  const [goalDialog, setGoalDialog] = useState<{ goal: Goal | null; groupId: number; presetStatus?: GoalStatus } | null>(null);
  const panel = usePanelSelection<string>();
  const [editing, setEditing] = useState<Task | PlacedTask | null>(null);
  const [creating, setCreating] = useState<NewTask | null>(null);
  const quickRef = useRef<HTMLInputElement>(null);
  const mut = useTodoMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();

  // Fall back to 今天 if the selected group / goal disappears (deleted, imported over).
  const group = view.kind === "group" ? groups.find((g) => g.id === view.id) : undefined;
  const goal = view.kind === "goal" ? goals.find((g) => g.id === view.id) : undefined;
  useEffect(() => {
    if (groupsPending || goalsPending) return;
    if ((view.kind === "group" && !group) || (view.kind === "goal" && !goal)) setView({ kind: "today" });
  }, [view, group, goal, groupsPending, goalsPending]);

  const place = view.kind === "group" ? { groupId: view.id } : view.kind === "goal" ? { goalId: view.id } : null;
  const placeQ = useTasks(place);
  const placeTasks = useMemo(() => placeQ.data ?? [], [placeQ.data]);


  const go = async (v: View) => {
    if (await panel.request(null)) setView(v);
  };
  const openEdit = async (t: Task) => {
    if (await panel.request(`t${t.id}`)) {
      setCreating(null);
      setEditing(t);
    }
  };
  const openNew = async (n: NewTask) => {
    if (await panel.request(`new${JSON.stringify(n)}`)) {
      setEditing(null);
      setCreating(n);
    }
  };
  // 全局搜索: a group / goal opens its view; a task opens its group or goal view, then its panel once
  // that view's tasks are loaded
  const [pendingTask, setPendingTask] = useState<number | null>(null);
  useOpenRequest(async (h) => {
    if (!(await panel.request(null))) return;
    if (h.kind === "group") setView({ kind: "group", id: h.id });
    else if (h.kind === "goal") setView({ kind: "goal", id: h.id });
    else if (h.parent !== null) {
      setView(h.parent < 0 ? { kind: "goal", id: -h.parent } : { kind: "group", id: h.parent });
      setPendingTask(h.id);
    }
  });
  useEffect(() => {
    const t = pendingTask !== null ? placeTasks.find((x) => x.id === pendingTask) : undefined;
    if (t) {
      setPendingTask(null);
      void openEdit(t);
    }
  }, [pendingTask, placeTasks]); // eslint-disable-line react-hooks/exhaustive-deps
  const whereHere = view.kind === "group" ? `g:${view.id}` : view.kind === "goal" ? `o:${view.id}` : `g:${todayGroupId ?? groups[0]?.id ?? ""}`;
  const addSub = (t: Task) => void openNew({ where: whereHere, parentId: t.id, parentTitle: t.title });

  // ⌘N: 今天 → the quick add; a group / goal → 新任务
  const newHere = view.kind === "group" || view.kind === "goal" ? () => void openNew({ where: whereHere }) : undefined;
  usePageKeys({
    onNew: groups.length > 0 ? (newHere ?? (() => quickRef.current?.focus())) : undefined,
    onEscape: panel.selection !== null ? () => void panel.request(null) : undefined,
  });

  const deleteEditing = async () => {
    const t = editing;
    if (!t) return;
    const subs = placeTasks.some((x) => x.parentId === t.id);
    const ok = await confirm({
      title: `删除「${t.title.replace(/\*\*/g, "")}」？`,
      message: subs ? "它下面的子任务会一起删除。" : undefined,
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    mut.deleteTask.mutate(t.id, {
      onSuccess: () => {
        toast("已删除");
        panel.done();
      },
    });
  };

  const counts = agenda ? { due: agenda.due.length, overdue: agenda.overdue.length } : null;
  const stats =
    counts && groups.length > 0 ? (
      <p className="pointer-events-none text-sm text-muted-foreground">
        今天 <b className="text-base text-foreground">{counts.due}</b> 件
        {counts.overdue > 0 && (
          <>
            {" "}
            · 已过期 <b className="text-base text-destructive">{counts.overdue}</b> 件
          </>
        )}
      </p>
    ) : null;

  const selectedId = panel.selection !== null ? (editing?.id ?? null) : null;
  const goalCount = (gid: number) => goals.filter((g) => g.groupId === gid).length;

  // ---------- views ----------

  const todayView = () => {
    const gid = todayGroupId ?? groups[0]?.id ?? null;
    return (
      <>
        <ViewHeader title={`今天 · ${formatDay(today)} ${weekday(today)}`} />
        <div className="-m-1 min-h-0 flex-1 space-y-5 overflow-y-auto p-1 pb-4">
          {agenda && agenda.overdue.length > 0 && (
            <Section title={`已过期 ${agenda.overdue.length}`} danger>
              <PlacedList items={agenda.overdue} today={today} selectedId={selectedId} onOpen={(t) => void openEdit(t)} />
            </Section>
          )}
          {agenda && agenda.due.length > 0 && (
            <Section title={`今天 ${agenda.due.length}`}>
              <PlacedList items={agenda.due} today={today} selectedId={selectedId} onOpen={(t) => void openEdit(t)} />
            </Section>
          )}
          {agenda && agenda.overdue.length + agenda.due.length === 0 && (
            <p className="py-10 text-center text-muted-foreground">今天没有安排，喝杯茶吧 🍵</p>
          )}
          {gid !== null && (
            <QuickAdd
              inputRef={quickRef}
              placeholder="添加今天要做的事"
              onAdd={(title) => mut.createTask.mutateAsync({ title, groupId: gid, dueDate: today })}
              extra={
                <Dropdown
                  ariaLabel="加到哪个分组"
                  value={gid}
                  onChange={setTodayGroupId}
                  options={groups.map((g) => ({ value: g.id, label: g.name }))}
                  align="end"
                  triggerClassName="inline-flex shrink-0 items-center gap-1 whitespace-nowrap rounded-control px-2 py-0.5 text-xs text-muted-foreground outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
                />
              }
            />
          )}
          {agenda && agenda.doneToday.length > 0 && (
            <Section title={`今天完成了 ${agenda.doneToday.length} 件`}>
              <PlacedList items={agenda.doneToday} today={today} selectedId={selectedId} onOpen={(t) => void openEdit(t)} />
            </Section>
          )}
        </div>
      </>
    );
  };

  const goalCard = (g: Goal) => (
    <button
      key={g.id}
      type="button"
      onClick={() => void go({ kind: "goal", id: g.id })}
      className="w-full space-y-1.5 rounded-control border-b px-3 py-2.5 text-left transition-colors duration-150 last:border-b-0 hover:bg-muted/70"
    >
      <div className="flex items-baseline justify-between gap-2">
        <span className={`flex min-w-0 items-center gap-1.5 ${closedGoal(g.status) ? "text-muted-foreground" : ""}`}>
          <span className="truncate">{g.name}</span>
          {g.status !== "active" && <span className="shrink-0 rounded bg-muted px-1.5 text-xs text-muted-foreground">{GOAL_STATUS[g.status]}</span>}
        </span>
        <span className="shrink-0 text-xs text-muted-foreground">
          {g.done}/{g.total}
          {(g.startDate || g.endDate) && ` · ${[formatDay(g.startDate), formatDay(g.endDate)].filter(Boolean).join(" – ")}`}
        </span>
      </div>
      <ProgressBar done={g.done} total={g.total} />
    </button>
  );

  const newTaskButton = (
    <Button onClick={() => newHere?.()}>
      <ListPlus size={14} /> 新任务
    </Button>
  );

  const groupView = (grp: TodoGroup) => {
    const gs = goals.filter((g) => g.groupId === grp.id);
    const openGoals = gs.filter((g) => !closedGoal(g.status));
    const doneGoals = gs.filter((g) => closedGoal(g.status));
    const openRoots = placeTasks.some((t) => t.parentId === null && !t.done);
    // finished goals sit in the same 「已完成」 fold as the finished tasks
    const tree = (
      <TaskTree
        tasks={placeTasks}
        today={today}
        selectedId={selectedId}
        onOpen={(t) => void openEdit(t)}
        onAddSub={addSub}
        doneGoals={{ count: doneGoals.length, node: <div className="rounded-card border bg-card">{doneGoals.map(goalCard)}</div> }}
      />
    );
    return (
      <>
        <ViewHeader
          title={grp.name}
          onEdit={() => setGroupDialog({ group: grp })}
          editLabel="编辑分组"
          sub={[openGoals.length > 0 && `${openGoals.length} 个目标`, `${grp.open} 件没做完`].filter(Boolean).join(" · ")}
          actions={
            <>
              <Button onClick={() => setGoalDialog({ goal: null, groupId: grp.id })}>
                <Target size={14} /> 新目标
              </Button>
              {newTaskButton}
            </>
          }
        />
        <div className="-m-1 min-h-0 flex-1 space-y-5 overflow-y-auto p-1 pb-4">
          {openGoals.length > 0 && (
            <Section title="目标">
              <div className="rounded-card border bg-card">{openGoals.map(goalCard)}</div>
            </Section>
          )}
          {openRoots ? (
            <Section title={openGoals.length > 0 ? "其他任务" : "任务"}>{tree}</Section>
          ) : placeTasks.length > 0 || doneGoals.length > 0 ? (
            tree
          ) : (
            gs.length === 0 && placeQ.data && <p className="py-10 text-center text-muted-foreground">这一组还空着</p>
          )}
        </div>
      </>
    );
  };

  const goalView = (g: Goal) => {
    const grp = groups.find((x) => x.id === g.groupId);
    const allDone = g.status === "active" && g.total > 0 && g.done === g.total;
    return (
      <>
        <ViewHeader
          title={
            <span className="flex items-center gap-2">
              {g.name}
              {g.status !== "active" && <span className="rounded bg-muted px-1.5 text-xs font-normal text-muted-foreground">{GOAL_STATUS[g.status]}</span>}
            </span>
          }
          onEdit={() => setGoalDialog({ goal: g, groupId: g.groupId })}
          editLabel="编辑目标"
          actions={newTaskButton}
          sub={[grp?.name, (g.startDate || g.endDate) && [formatDay(g.startDate), formatDay(g.endDate)].filter(Boolean).join(" – "), `${g.done} / ${g.total} · ${pct(g.done, g.total)}%`]
            .filter(Boolean)
            .join(" · ")}
        />
        <div className="-m-1 min-h-0 flex-1 space-y-4 overflow-y-auto p-1 pb-4">
          <ProgressBar done={g.done} total={g.total} />
          {allDone && (
            <div className="flex items-center justify-between gap-3 rounded-card border bg-card px-4 py-2.5 text-sm">
              <span>全部完成了 🎉 要把这个目标标为「已完成」，写几句复盘吗？</span>
              <Button variant="primary" onClick={() => setGoalDialog({ goal: g, groupId: g.groupId, presetStatus: "done" })}>
                标为已完成
              </Button>
            </div>
          )}
          {g.note && <p className="selectable px-1 text-sm whitespace-pre-wrap text-muted-foreground"><InlineMd text={g.note} /></p>}
          {g.review && (
            <Section title="复盘">
              <p className="selectable rounded-card border bg-card px-4 py-2.5 text-sm whitespace-pre-wrap">{g.review}</p>
            </Section>
          )}
          <TaskTree
            tasks={placeTasks}
            today={today}
            selectedId={selectedId}
            onOpen={(t) => void openEdit(t)}
            onAddSub={addSub}
          />
          {placeQ.data && placeTasks.length === 0 && <p className="py-10 text-center text-muted-foreground">还没有任务</p>}
        </div>
      </>
    );
  };

  // ---------- layout ----------

  const nav = (
    <nav aria-label="一步一步" className="flex w-60 shrink-0 flex-col">
      <div className="-m-1 min-h-0 flex-1 space-y-0.5 overflow-y-auto p-1 pr-2">
        <NavButton active={view.kind === "today"} onClick={() => void go({ kind: "today" })}>
          <Sun size={15} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
          <span className="flex-1">今天</span>
          {counts && counts.overdue > 0 && <span className="text-xs text-destructive">{counts.overdue} 过期</span>}
          {counts && <span className="text-xs text-muted-foreground">{counts.due}</span>}
        </NavButton>
        <NavButton active={view.kind === "month"} onClick={() => void go({ kind: "month" })}>
          <CalendarDays size={15} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
          <span className="flex-1">月历</span>
        </NavButton>
        <NavButton active={view.kind === "review"} onClick={() => void go({ kind: "review" })}>
          <CalendarCheck size={15} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
          <span className="flex-1">回看</span>
        </NavButton>
        <div className="px-2.5 pt-4 pb-1 text-xs text-muted-foreground">分组</div>
        <GroupNav groups={groups} goals={goals} view={view} go={(v) => void go(v)} />
      </div>
      <Button variant="ghost" className="mt-2 justify-center text-muted-foreground" onClick={() => setGroupDialog({ group: null })}>
        <FolderPlus size={14} /> 新分组
      </Button>
    </nav>
  );

  const parentTitle =
    editing === null ? null : "parentTitle" in editing ? editing.parentTitle : (placeTasks.find((t) => t.id === editing.parentId)?.title ?? null);
  // Selection and editing/creating are set in two steps; only mount once both agree, or a panel could
  // start from the previous task's draft.
  const panelOpen =
    panel.selection !== null &&
    ((editing !== null && panel.selection === `t${editing.id}`) || (creating !== null && panel.selection === `new${JSON.stringify(creating)}`));
  // The panel shows live counters (check-ins, amounts): take the latest copy of the task we have.
  const fresh =
    editing &&
    ([...placeTasks, ...(agenda ? [...agenda.overdue, ...agenda.due, ...agenda.doneToday] : [])].find((t) => t.id === editing.id) ?? editing);

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader icon={m.icon} color={m.color} title={m.label} right={stats} />
      {groupsPending ? null : groups.length === 0 ? (
        <EmptyState
          title="一步一步，先建一个分组吧"
          action={
            <Button variant="primary" onClick={() => setGroupDialog({ group: null })}>
              <FolderPlus size={14} /> 新分组
            </Button>
          }
        />
      ) : (
        <div className="flex min-h-0 flex-1 gap-6 px-8 pb-6">
          {nav}
          <section className="flex min-w-0 flex-1 flex-col">
            {view.kind === "today" && todayView()}
            {view.kind === "month" && <MonthView today={today} selectedId={selectedId} onOpen={(t) => void openEdit(t)} />}
            {view.kind === "review" && <ReviewView today={today} selectedId={selectedId} onOpen={(t) => void openEdit(t)} />}
            {group && groupView(group)}
            {goal && goalView(goal)}
          </section>
        </div>
      )}

      {panelOpen && (
        <TaskPanel
          key={panel.selection}
          task={fresh}
          create={creating ?? undefined}
          parentTitle={parentTitle}
          hasChildren={!!editing && placeTasks.some((t) => t.parentId === editing.id)}
          groups={groups}
          goals={goals}
          onClose={() => void panel.request(null)}
          onDone={panel.done}
          onDirtyChange={panel.setDirty}
          onDelete={editing ? () => void deleteEditing() : undefined}
        />
      )}
      {groupDialog && (
        <GroupDialog
          group={groupDialog.group}
          goalCount={groupDialog.group ? goalCount(groupDialog.group.id) : 0}
          onClose={() => setGroupDialog(null)}
          onCreated={(g) => void go({ kind: "group", id: g.id })}
          onDeleted={() => setView({ kind: "today" })}
        />
      )}
      {goalDialog && (
        <GoalDialog
          goal={goalDialog.goal}
          groupId={goalDialog.groupId}
          groups={groups}
          presetStatus={goalDialog.presetStatus}
          onClose={() => setGoalDialog(null)}
          onCreated={(g) => void go({ kind: "goal", id: g.id })}
          onDeleted={() => setView({ kind: "group", id: goalDialog.groupId })}
        />
      )}
      {panel.confirmDialog}
      {confirmDialog}
    </div>
  );
}
