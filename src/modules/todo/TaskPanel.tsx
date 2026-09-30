// Side panel to create / edit a task, including its type (普通 / 重复 / 打卡 / 进度条). Root tasks
// can move between groups and goals; subtasks stay under their parent. For saved tasks the type's
// records (skip / check-in calendar / amounts) act immediately and are undoable with ⌘Z.
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { ChevronLeft, ChevronRight, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { addDays, formatDay, localToday, mondayOf, parseYmd, toYmd } from "@/shared/dates";
import { errorText, useToast } from "@/shared/feedback";
import { SidePanel } from "@/shared/panel";
import { Button, Field, IconButton, inputClass, Segmented, TextArea, TextInput } from "@/shared/ui";
import { PERIODS, TASK_TYPES, useLogs, useTodoMutations, type Goal, type Period, type Task, type TaskPatch, type TaskType, type TodoGroup } from "./api";

type Deadline = "none" | "due" | "range";
type Draft = {
  title: string;
  where: string;
  planDate: string;
  deadline: Deadline;
  startDate: string;
  dueDate: string;
  link: string;
  note: string;
  taskType: TaskType;
  rule: string;
  repeatTotal: string;
  repeatUntil: string;
  target: string;
  unit: string;
  period: Period;
};

/** Where a new task goes. `where` = "g:<groupId>" or "o:<goalId>". */
export type NewTask = { where: string; parentId?: number; parentTitle?: string; planDate?: string };

/** UTC timestamp → local "10 月 1 日". */
const localDay = (ts: string) => formatDay(toYmd(new Date(ts)));
const whereOf = (t: Task) => (t.goalId !== null ? `o:${t.goalId}` : `g:${t.groupId}`);
const placeOf = (where: string): Pick<TaskPatch, "groupId" | "goalId"> => {
  const id = Number(where.slice(2));
  return where.startsWith("o:") ? { goalId: id } : { groupId: id, goalId: null };
};
const WEEK = ["一", "二", "三", "四", "五", "六", "日"];

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

const NumberInput = (props: { label: string; value: string; onChange: (v: string) => void; placeholder?: string }) => (
  <input
    type="number"
    min={1}
    aria-label={props.label}
    value={props.value}
    placeholder={props.placeholder}
    onChange={(e) => props.onChange(e.target.value)}
    className={`${inputClass} w-24`}
  />
);

function Weekdays(props: { value: number[]; onChange: (v: number[]) => void }) {
  return (
    <div className="flex gap-1" role="group" aria-label="星期几">
      {WEEK.map((w, i) => {
        const d = i + 1;
        const on = props.value.includes(d);
        return (
          <button
            key={d}
            type="button"
            aria-pressed={on}
            onClick={() => props.onChange(on ? props.value.filter((x) => x !== d) : [...props.value, d].sort())}
            className={`size-8 rounded-full text-sm transition-colors ${on ? "bg-primary text-primary-foreground" : "bg-muted text-muted-foreground hover:text-foreground"}`}
          >
            {w}
          </button>
        );
      })}
    </div>
  );
}

type Mode = "daily" | "weekly" | "monthly" | "every" | "after";

/** Repeat rule builder. `checkin` limits it to 每天 / 每周几天. */
function RuleEditor(props: { value: string; onChange: (v: string) => void; checkin?: boolean }) {
  const [kind, arg = ""] = props.value.split(":");
  const mode = (["daily", "weekly", "monthly", "every", "after"].includes(kind) ? kind : "daily") as Mode;
  const days = mode === "weekly" ? arg.split(",").filter(Boolean).map(Number) : [];
  const n = mode === "every" || mode === "after" ? arg.slice(0, -1) : mode === "monthly" ? arg : "";
  const unit = mode === "every" && arg.endsWith("w") ? "w" : "d";
  const modes: { value: Mode; label: string }[] = props.checkin
    ? [
        { value: "daily", label: "每天" },
        { value: "weekly", label: "每周几天" },
      ]
    : [
        { value: "daily", label: "每天" },
        { value: "weekly", label: "每周" },
        { value: "monthly", label: "每月" },
        { value: "every", label: "每隔" },
        { value: "after", label: "完成后" },
      ];
  const setMode = (m: Mode) =>
    props.onChange({ daily: "daily", weekly: `weekly:${days.length ? days.join(",") : "1"}`, monthly: "monthly:1", every: "every:2d", after: "after:30d" }[m]);
  return (
    <div className="space-y-2">
      <Segmented ariaLabel="重复方式" value={mode} onChange={setMode} options={modes} />
      {mode === "weekly" && <Weekdays value={days} onChange={(ds) => props.onChange(`weekly:${ds.join(",")}`)} />}
      {mode === "monthly" && (
        <div className="flex items-center gap-2 text-sm">
          每月 <NumberInput label="几号" value={n} onChange={(v) => props.onChange(`monthly:${v}`)} /> 号（没有这天的月份用最后一天）
        </div>
      )}
      {mode === "every" && (
        <div className="flex items-center gap-2 text-sm">
          每隔 <NumberInput label="间隔" value={n} onChange={(v) => props.onChange(`every:${v}${unit}`)} />
          <Segmented
            ariaLabel="单位"
            value={unit}
            onChange={(u) => props.onChange(`every:${n}${u}`)}
            options={[
              { value: "d", label: "天" },
              { value: "w", label: "周" },
            ]}
          />
        </div>
      )}
      {mode === "after" && (
        <div className="flex items-center gap-2 text-sm">
          完成后过 <NumberInput label="天数" value={n} onChange={(v) => props.onChange(`after:${v}d`)} /> 天再出现
        </div>
      )}
    </div>
  );
}

/** Month grid of a check-in task: click a scheduled past day to check in / un-check (补打卡). */
function CheckinCalendar(props: { task: Task }) {
  const t = props.task;
  const today = localToday();
  const [month, setMonth] = useState(today.slice(0, 7));
  const { data: logs = [] } = useLogs(t.id);
  const m = useTodoMutations();
  const checked = new Set(logs.map((l) => l.day));
  const first = `${month}-01`;
  const gridStart = mondayOf(first);
  const [y, mo] = month.split("-").map(Number);
  const cells = Array.from({ length: 42 }, (_, i) => addDays(gridStart, i)).filter((d, i) => i < 35 || d.startsWith(month));
  const [kind, arg = ""] = (t.repeatRule ?? "daily").split(":");
  const scheduled = (d: string) => kind === "daily" || arg.split(",").map(Number).includes(((parseYmd(d).getDay() + 6) % 7) + 1);
  const shift = (k: number) => {
    const d = new Date(y, mo - 1 + k, 1);
    setMonth(toYmd(d).slice(0, 7));
  };
  return (
    <div className="rounded-card border p-3">
      <div className="mb-2 flex items-center justify-between text-sm">
        <IconButton label="上个月" onClick={() => shift(-1)}>
          <ChevronLeft size={14} />
        </IconButton>
        <span>
          {y} 年 {mo} 月 · 连续 {t.streak} 天 · 共 {t.count} 天
        </span>
        <IconButton label="下个月" disabled={month >= today.slice(0, 7)} onClick={() => shift(1)}>
          <ChevronRight size={14} />
        </IconButton>
      </div>
      <div className="grid grid-cols-7 gap-1 text-center text-xs">
        {WEEK.map((w) => (
          <span key={w} className="text-muted-foreground">
            {w}
          </span>
        ))}
        {cells.map((d) => {
          const inMonth = d.startsWith(month);
          const can = inMonth && scheduled(d) && d <= today && (!t.planDate || d >= t.planDate);
          const on = checked.has(d);
          return (
            <button
              key={d}
              type="button"
              disabled={!can}
              aria-pressed={on}
              aria-label={`${formatDay(d)}${on ? "，已打卡" : ""}`}
              onClick={() => m.log.mutate({ taskId: t.id, day: d })}
              className={`aspect-square rounded-full transition-colors ${!inMonth ? "invisible" : ""} ${
                on ? "text-primary-foreground" : can ? "hover:bg-muted" : "text-muted-foreground/40"
              } ${d === today ? "ring-1 ring-ring" : ""}`}
              style={on ? { background: "var(--status-done-fg)" } : undefined}
            >
              {Number(d.slice(8))}
            </button>
          );
        })}
      </div>
      <p className="mt-2 text-xs text-muted-foreground">点日期补打卡，再点一次取消。</p>
    </div>
  );
}

/** Progress task: count an amount on a day; recent entries can be removed. */
function ProgressLog(props: { task: Task }) {
  const t = props.task;
  const today = localToday();
  const [amount, setAmount] = useState("1");
  const [day, setDay] = useState(today);
  const { data: logs = [] } = useLogs(t.id);
  const m = useTodoMutations();
  const unit = t.unit ?? "";
  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        <NumberInput label="数量" value={amount} onChange={setAmount} />
        <span className="text-sm">{unit}</span>
        <input type="date" aria-label="日期" value={day} max={today} onChange={(e) => setDay(e.target.value)} className={`${inputClass} w-auto`} />
        <Button onClick={() => m.log.mutate({ taskId: t.id, day, amount: Number(amount) || 1 })}>记上</Button>
      </div>
      <p className="text-xs text-muted-foreground">
        累计 {t.totalAmount} {unit}
        {t.period !== "all" && `，${PERIODS[t.period ?? "all"]}目标 ${t.target} ${unit}`}
      </p>
      {logs.length > 0 && (
        <ul className="max-h-40 overflow-y-auto rounded-card border text-sm">
          {logs.slice(0, 30).map((l) => (
            <li key={l.id} className="flex items-center justify-between border-b px-3 py-1 last:border-b-0">
              <span>
                {formatDay(l.day)} · +{l.amount} {unit}
              </span>
              <IconButton label="删除这条记录" danger onClick={() => m.deleteLog.mutate(l.id)}>
                <X size={12} />
              </IconButton>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function TaskPanel(props: {
  task: Task | null; // null = create
  create?: NewTask;
  parentTitle: string | null;
  hasChildren: boolean;
  groups: TodoGroup[];
  goals: Goal[];
  onClose: () => void;
  onDone: () => void;
  onDirtyChange: (dirty: boolean) => void;
  onDelete?: () => void;
}) {
  const { task } = props;
  const today = localToday();
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
      taskType: task?.taskType ?? "normal",
      rule: task?.repeatRule ?? "daily",
      repeatTotal: task?.repeatTotal?.toString() ?? "",
      repeatUntil: task?.repeatUntil ?? "",
      target: task?.target?.toString() ?? "",
      unit: task?.unit ?? "",
      period: task?.period ?? "all",
    }),
    [task, props.create],
  );
  const [d, setD] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const m = useTodoMutations();
  const toast = useToast();
  const isSub = task ? task.parentId !== null : props.create?.parentId !== undefined;

  // Dates can move underneath the panel (an occurrence finished / skipped from the list): follow
  // them unless the user already edited that field.
  const prevInitial = useRef(initial);
  useEffect(() => {
    const p = prevInitial.current;
    prevInitial.current = initial;
    setD((x) => {
      const next = { ...x };
      for (const k of ["planDate", "startDate", "dueDate", "deadline"] as const) if (x[k] === p[k]) (next[k] as string) = initial[k];
      return next;
    });
  }, [initial]);

  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps

  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));
  const setType = (v: TaskType) =>
    setD((x) => ({
      ...x,
      taskType: v,
      rule: v === "checkin" && !/^(daily|weekly)/.test(x.rule) ? "daily" : x.rule,
      planDate: (v === "checkin" || v === "repeat") && !x.planDate && !x.dueDate ? today : x.planDate,
    }));

  const whereOptions = props.groups.flatMap((g) => [
    { value: `g:${g.id}`, label: g.name },
    ...props.goals.filter((o) => o.groupId === g.id).map((o) => ({ value: `o:${o.id}`, label: `　◎ ${o.name}` })),
  ]);

  const num = (s: string) => (s.trim() ? Number(s) : null);
  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const type = d.taskType;
    const fields: TaskPatch = {
      title: d.title,
      note: d.note.trim() || null,
      link: d.link.trim() || null,
      planDate: d.planDate || null,
      startDate: type !== "checkin" && d.deadline === "range" ? d.startDate || null : null,
      dueDate: type === "checkin" || d.deadline === "none" ? null : d.dueDate || null,
      taskType: type,
      repeatRule: type === "repeat" || type === "checkin" ? d.rule : null,
      repeatTotal: type === "repeat" || type === "checkin" ? num(d.repeatTotal) : null,
      repeatUntil: type === "repeat" || type === "checkin" ? d.repeatUntil || null : null,
      target: type === "progress" ? num(d.target) : null,
      unit: type === "progress" ? d.unit.trim() || null : null,
      period: type === "progress" ? d.period : null,
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

  const typeLocked = props.hasChildren; // only normal tasks can have subtasks
  const saved = task !== null && task.taskType === d.taskType; // type records only for the saved type
  return (
    <SidePanel
      title={task ? "编辑任务" : isSub ? "添加子任务" : "添加任务"}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={props.onDelete}
      error={error}
      meta={
        task
          ? [`添加于 ${localDay(task.createdAt)}`, task.done && (task.doneAt ? `完成于 ${localDay(task.doneAt)}` : "已完成")].filter(Boolean).join(" · ")
          : undefined
      }
    >
      <Field label="任务">
        <TextInput autoFocus value={d.title} onChange={(e) => set("title", e.target.value)} placeholder="必填" maxLength={500} />
      </Field>
      <Field label="类型">
        {typeLocked ? (
          <p className="text-sm text-muted-foreground">有子任务，只能是普通任务</p>
        ) : (
          <Segmented ariaLabel="类型" value={d.taskType} onChange={setType} options={(Object.keys(TASK_TYPES) as TaskType[]).map((k) => ({ value: k, label: TASK_TYPES[k] }))} />
        )}
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

      {d.taskType === "repeat" && (
        <>
          <Field label="重复">
            <RuleEditor value={d.rule} onChange={(v) => set("rule", v)} />
          </Field>
          <div className="flex gap-4">
            <Field label="一共几次">
              <NumberInput label="一共几次" value={d.repeatTotal} onChange={(v) => set("repeatTotal", v)} placeholder="不限" />
            </Field>
            <Field label="到哪天结束">
              <DateInput label="结束日期" value={d.repeatUntil} onChange={(v) => set("repeatUntil", v)} />
            </Field>
          </div>
          {saved && !task.done && (
            <div className="flex items-center justify-between gap-2 rounded-card border px-3 py-2 text-sm">
              <span>已完成 {task.count} 次，现在是第 {task.count + 1} 次</span>
              <Button
                onClick={() =>
                  // dates move on in Rust: close so a stale draft can't save the old dates back
                  m.skip.mutate(task.id, {
                    onSuccess: () => {
                      toast("已跳过这一次（⌘Z 撤销）");
                      props.onDone();
                    },
                  })
                }
              >
                跳过这一次
              </Button>
            </div>
          )}
        </>
      )}
      {d.taskType === "checkin" && (
        <>
          <Field label="哪些天打卡">
            <RuleEditor checkin value={d.rule} onChange={(v) => set("rule", v)} />
          </Field>
          <div className="flex gap-4">
            <Field label="开始日期">
              <DateInput label="开始日期" value={d.planDate} onChange={(v) => set("planDate", v)} />
            </Field>
            <Field label="目标天数">
              <NumberInput label="目标天数" value={d.repeatTotal} onChange={(v) => set("repeatTotal", v)} placeholder="不限" />
            </Field>
          </div>
          <Field label="结束日期">
            <DateInput label="结束日期" value={d.repeatUntil} onChange={(v) => set("repeatUntil", v)} />
          </Field>
          {saved && <CheckinCalendar task={task} />}
        </>
      )}
      {d.taskType === "progress" && (
        <>
          <div className="flex gap-4">
            <Field label="目标数量">
              <NumberInput label="目标数量" value={d.target} onChange={(v) => set("target", v)} placeholder="必填" />
            </Field>
            <Field label="单位">
              <TextInput value={d.unit} onChange={(e) => set("unit", e.target.value)} placeholder="本、份、次…" maxLength={10} className="w-28" />
            </Field>
          </div>
          <Field label="怎么计数">
            <Segmented ariaLabel="计数周期" value={d.period} onChange={(v) => set("period", v)} options={(Object.keys(PERIODS) as Period[]).map((k) => ({ value: k, label: PERIODS[k] }))} />
          </Field>
          {saved && (
            <Field label="记一笔">
              <ProgressLog task={task} />
            </Field>
          )}
        </>
      )}

      {d.taskType !== "checkin" && (
        <>
          <Field label={d.taskType === "repeat" ? "计划哪天做（这一次）" : "计划哪天做"}>
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
        </>
      )}
      <Field label="链接">
        <TextInput value={d.link} onChange={(e) => set("link", e.target.value)} placeholder="https://…" maxLength={1000} />
      </Field>
      <Field label="备注">
        <TextArea rows={4} value={d.note} onChange={(e) => set("note", e.target.value)} placeholder="用 **两个星号** 包起来就是加粗" maxLength={5000} />
      </Field>
    </SidePanel>
  );
}
