// 一步一步: left nav (今天 · 回看 · groups → goals) + right content. Modeled on GoalMap.
import { useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { CalendarCheck, ChevronLeft, ChevronRight, FolderPlus, Pencil, Plus, Sun, Target } from "lucide-react";
import { moduleById } from "@/app/modules";
import { Dropdown } from "@/shared/Dropdown";
import { addDays, formatDay, localToday, mondayOf, toYmd, weekday } from "@/shared/dates";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { InlineMd } from "@/shared/InlineMd";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { Button, EmptyState, IconButton, PageHeader, ProgressBar } from "@/shared/ui";
import {
  GOAL_STATUS,
  pct,
  useAgenda,
  useDoneBetween,
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
import { PlacedList, TaskTree } from "./TaskList";
import { TaskPanel, type NewTask } from "./TaskPanel";

type View = { kind: "today" } | { kind: "review" } | { kind: "group"; id: number } | { kind: "goal"; id: number };

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

function NavButton(props: { active: boolean; onClick: () => void; children: ReactNode; indent?: boolean }) {
  return (
    <button
      type="button"
      aria-current={props.active}
      onClick={props.onClick}
      className={`flex w-full items-center gap-2 rounded-control py-1.5 text-left transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${
        props.indent ? "pr-2.5 pl-6" : "px-2.5"
      } ${props.active ? "bg-accent" : "hover:bg-muted/70"}`}
    >
      {props.children}
    </button>
  );
}

const closedGoal = (s: GoalStatus) => s === "done" || s === "abandoned";

function GoalNavLabel({ goal }: { goal: Goal }) {
  const closed = closedGoal(goal.status);
  return (
    <>
      <Target size={13} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
      <span className={`min-w-0 flex-1 truncate text-sm ${closed ? "text-muted-foreground" : ""} ${goal.status === "abandoned" ? "line-through" : ""}`}>
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

function ViewHeader(props: { title: ReactNode; sub?: ReactNode; onEdit?: () => void; editLabel?: string; actions?: ReactNode }) {
  return (
    <div className="flex shrink-0 items-start justify-between gap-4 pb-3">
      <div className="min-w-0">
        <div className="flex items-center gap-1">
          <h2 className="truncate text-lg font-bold">{props.title}</h2>
          {props.onEdit && (
            <IconButton label={props.editLabel ?? "编辑"} onClick={props.onEdit}>
              <Pencil size={14} />
            </IconButton>
          )}
        </div>
        {props.sub && <div className="text-sm text-muted-foreground">{props.sub}</div>}
      </div>
      {props.actions && <div className="flex shrink-0 gap-2">{props.actions}</div>}
    </div>
  );
}

function Section(props: { title: ReactNode; danger?: boolean; children: ReactNode }) {
  return (
    <section>
      <h3 className={`mb-1.5 px-1 text-sm ${props.danger ? "text-destructive" : "text-muted-foreground"}`}>{props.title}</h3>
      {props.children}
    </section>
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
  const [weekOffset, setWeekOffset] = useState(0);
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

  const week = { from: addDays(mondayOf(today), weekOffset * 7), to: addDays(mondayOf(today), weekOffset * 7 + 6) };
  const doneQ = useDoneBetween(week.from, week.to);

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
  const whereHere = view.kind === "group" ? `g:${view.id}` : view.kind === "goal" ? `o:${view.id}` : `g:${todayGroupId ?? groups[0]?.id ?? ""}`;
  const addSub = (t: Task) => void openNew({ where: whereHere, parentId: t.id, parentTitle: t.title });

  usePageKeys({
    onNew: groups.length > 0 ? () => quickRef.current?.focus() : undefined,
    onEscape: panel.selection !== null ? () => void panel.request(null) : undefined,
  });

  const deleteEditing = async () => {
    const t = editing;
    if (!t) return;
    const subs = placeTasks.some((x) => x.parentId === t.id);
    const ok = await confirm({
      title: `删除「${t.title.replace(/\*\*/g, "")}」？`,
      message: `${subs ? "它下面的子任务会一起删除。" : ""}删除后只能从备份里找回。`,
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
        <div className="min-h-0 flex-1 space-y-5 overflow-y-auto pb-4">
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
              placeholder="添加今天要做的事，回车保存"
              onAdd={(title) => mut.createTask.mutateAsync({ title, groupId: gid, planDate: today })}
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

  const reviewView = () => {
    const items = doneQ.data ?? [];
    const byDay = new Map<string, PlacedTask[]>();
    for (const t of [...items].reverse()) {
      const day = toYmd(new Date(t.doneAt!));
      byDay.set(day, [...(byDay.get(day) ?? []), t]);
    }
    const label = weekOffset === 0 ? "本周" : weekOffset === -1 ? "上周" : `${formatDay(week.from)} 那一周`;
    return (
      <>
        <ViewHeader
          title="回看"
          sub={`${label} · ${formatDay(week.from)} – ${formatDay(week.to)} · 完成了 ${items.length} 件`}
          actions={
            <>
              <IconButton label="上一周" onClick={() => setWeekOffset((w) => w - 1)}>
                <ChevronLeft size={16} />
              </IconButton>
              <Button variant="ghost" disabled={weekOffset === 0} onClick={() => setWeekOffset(0)}>
                本周
              </Button>
              <IconButton label="下一周" disabled={weekOffset >= 0} onClick={() => setWeekOffset((w) => w + 1)}>
                <ChevronRight size={16} />
              </IconButton>
            </>
          }
        />
        <div className="min-h-0 flex-1 space-y-5 overflow-y-auto pb-4">
          {items.length === 0 && !doneQ.isPending ? (
            <p className="py-10 text-center text-muted-foreground">这一周还没有完成的事</p>
          ) : (
            [...byDay.entries()].map(([day, ts]) => (
              <Section key={day} title={`${formatDay(day)} ${weekday(day)} · ${ts.length} 件`}>
                <PlacedList items={ts} today={today} selectedId={selectedId} onOpen={(t) => void openEdit(t)} />
              </Section>
            ))
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
          <Target size={14} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
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

  const groupView = (grp: TodoGroup) => {
    const gs = goals.filter((g) => g.groupId === grp.id);
    return (
      <>
        <ViewHeader
          title={grp.name}
          onEdit={() => setGroupDialog({ group: grp })}
          editLabel="编辑分组"
          sub={[gs.length > 0 && `${gs.length} 个目标`, `${grp.open} 件没做完`].filter(Boolean).join(" · ")}
          actions={
            <Button onClick={() => setGoalDialog({ goal: null, groupId: grp.id })}>
              <Target size={14} /> 新目标
            </Button>
          }
        />
        <div className="min-h-0 flex-1 space-y-5 overflow-y-auto pb-4">
          {gs.length > 0 && (
            <Section title="目标">
              <div className="rounded-card border bg-card">{gs.map(goalCard)}</div>
            </Section>
          )}
          <Section title={gs.length > 0 ? "其他任务" : "任务"}>
            <div className="space-y-3">
              <TaskTree tasks={placeTasks} today={today} selectedId={selectedId} onOpen={(t) => void openEdit(t)} onAddSub={addSub} />
              <QuickAdd inputRef={quickRef} placeholder="添加任务，回车保存" onAdd={(title) => mut.createTask.mutateAsync({ title, groupId: grp.id })} />
            </div>
          </Section>
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
          sub={[grp?.name, (g.startDate || g.endDate) && [formatDay(g.startDate), formatDay(g.endDate)].filter(Boolean).join(" – "), `${g.done} / ${g.total} · ${pct(g.done, g.total)}%`]
            .filter(Boolean)
            .join(" · ")}
        />
        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pb-4">
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
            empty={<p className="px-1 text-sm text-muted-foreground">把目标拆成一步一步的任务吧</p>}
          />
          <QuickAdd inputRef={quickRef} placeholder="添加任务，回车保存" onAdd={(title) => mut.createTask.mutateAsync({ title, goalId: g.id })} />
        </div>
      </>
    );
  };

  // ---------- layout ----------

  const nav = (
    <nav aria-label="一步一步" className="flex w-60 shrink-0 flex-col">
      <div className="min-h-0 flex-1 space-y-0.5 overflow-y-auto pr-1">
        <NavButton active={view.kind === "today"} onClick={() => void go({ kind: "today" })}>
          <Sun size={15} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
          <span className="flex-1">今天</span>
          {counts && counts.overdue > 0 && <span className="text-xs text-destructive">{counts.overdue} 过期</span>}
          {counts && <span className="text-xs text-muted-foreground">{counts.due}</span>}
        </NavButton>
        <NavButton active={view.kind === "review"} onClick={() => void go({ kind: "review" })}>
          <CalendarCheck size={15} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
          <span className="flex-1">回看</span>
        </NavButton>
        <div className="px-2.5 pt-4 pb-1 text-xs text-muted-foreground">分组</div>
        {groups.map((g) => (
          <div key={g.id}>
            <NavButton active={view.kind === "group" && view.id === g.id} onClick={() => void go({ kind: "group", id: g.id })}>
              <span className="min-w-0 flex-1 truncate">{g.name}</span>
              <span className="shrink-0 text-xs text-muted-foreground">{g.open || ""}</span>
            </NavButton>
            {goals
              .filter((o) => o.groupId === g.id)
              .map((o) => (
                <NavButton key={o.id} indent active={view.kind === "goal" && view.id === o.id} onClick={() => void go({ kind: "goal", id: o.id })}>
                  <GoalNavLabel goal={o} />
                </NavButton>
              ))}
          </div>
        ))}
      </div>
      <Button variant="ghost" className="mt-2 justify-center text-muted-foreground" onClick={() => setGroupDialog({ group: null })}>
        <FolderPlus size={14} /> 新分组
      </Button>
    </nav>
  );

  const parentTitle =
    editing === null ? null : "parentTitle" in editing ? editing.parentTitle : (placeTasks.find((t) => t.id === editing.parentId)?.title ?? null);

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader icon={m.icon} color={m.color} title={m.label} right={stats} />
      {groupsPending ? null : groups.length === 0 ? (
        <EmptyState
          title="一步一步，先建一个分组吧"
          hint="比如「📌 Pick Up」「🖥️ Diploma」；也可以在「设置 → 导入」里导入整理好的待办"
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
            {view.kind === "review" && reviewView()}
            {group && groupView(group)}
            {goal && goalView(goal)}
          </section>
        </div>
      )}

      {panel.selection !== null && (editing || creating) && (
        <TaskPanel
          key={panel.selection}
          task={editing}
          create={creating ?? undefined}
          parentTitle={parentTitle}
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
