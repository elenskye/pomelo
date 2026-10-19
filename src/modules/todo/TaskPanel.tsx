// Side panel to create / edit a task: 标题, 内容, type (普通 / 重复 / 打卡 / 进度条) and its fields.
// Root tasks can move between groups and goals; subtasks stay under their parent. Dates per type:
// 普通 / 打卡 / 进度条 = 时间: 不设 or a 时间段; 重复 = none to enter (Rust derives each occurrence
// from the rule and the counts), only 完成后 takes a start date. For saved tasks the type's records
// (skip / check-in calendar / amounts) act immediately. 小拾: 润色 on 内容; a saved 普通 task can be split
// into subtasks to pick from (「加上」 adds them as one undo step). A task made from a note links back to it.
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight, Feather, X } from "lucide-react";
import type { Note } from "@/modules/notes/api";
import { excerpt } from "@/modules/notes/NoteBody";
import { AssistAction, IdeaList, Polish, QuickFill, assistApi, assistBusyText, useAssistOn, useFilled, type QuickFillResult } from "@/modules/picky/Assist";
import { usePickyStatus } from "@/modules/picky/api";
import { useAttachmentsOf, type FileInfo } from "@/shared/attachments/api";
import { AttachmentField } from "@/shared/attachments/Attachments";
import { DateInput } from "@/shared/DateInput";
import { Dropdown } from "@/shared/Dropdown";
import { addDays, formatDay, localToday, mondayOf, parseYmd, slashDate, toYmd } from "@/shared/dates";
import { errorText, useToast } from "@/shared/feedback";
import { SidePanel } from "@/shared/panel";
import { Button, Field, IconButton, fieldClass, Segmented, TextArea, TextInput } from "@/shared/ui";
import { TASK_TYPES, useLogs, useTodoMutations, type Goal, type Task, type TaskPatch, type TaskType, type TodoGroup } from "./api";

type Draft = {
  title: string;
  note: string;
  where: string;
  taskType: TaskType;
  ranged: boolean; // 普通 / 打卡 / 进度条: 时间段 or 不设
  startDate: string;
  dueDate: string;
  rule: string;
  planDate: string; // 重复 (完成后): start
  repeatTotal: string;
  doneCount: string; // 重复 (all but 完成后)
  target: string;
  unit: string;
  link: string;
  files: FileInfo[];
};

/** Where a new task goes. `where` = "g:<groupId>" or "o:<goalId>". 转成待办 fills title / note and links the note. */
export type NewTask = { where: string; parentId?: number; parentTitle?: string; dueDate?: string; title?: string; note?: string; fromNote?: number };

/** UTC timestamp → local "10 月 1 日". */
const localDay = (ts: string) => formatDay(toYmd(new Date(ts)));
const whereOf = (t: Task) => (t.goalId !== null ? `o:${t.goalId}` : `g:${t.groupId}`);
const placeOf = (where: string): Pick<TaskPatch, "groupId" | "goalId"> => {
  const id = Number(where.slice(2));
  return where.startsWith("o:") ? { goalId: id } : { groupId: id, goalId: null };
};
const WEEK = ["一", "二", "三", "四", "五", "六", "日"];
const ruleKind = (rule: string) => rule.split(":")[0];
/** Rules whose series ends by count (一共几次 / 已经做了几次); 完成后 runs from a start date instead. */
const counted = (rule: string) => ruleKind(rule) !== "after";

/** One row: start (clearable: a deadline only) 到 end. */
function RangeInput(props: { start: string; end: string; onStart: (v: string) => void; onEnd: (v: string) => void }) {
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <DateInput clearable ariaLabel="开始日期" value={props.start} onChange={props.onStart} max={props.end || undefined} />
      <span className="shrink-0 text-muted-foreground">到</span>
      <DateInput ariaLabel="结束日期" value={props.end} onChange={props.onEnd} min={props.start || undefined} />
    </div>
  );
}

const NumberInput = (props: { label: string; value: string; onChange: (v: string) => void; placeholder?: string; min?: number }) => (
  <input
    type="number"
    min={props.min ?? 1}
    aria-label={props.label}
    value={props.value}
    placeholder={props.placeholder}
    onChange={(e) => props.onChange(e.target.value)}
    className={`${fieldClass} w-24`}
  />
);

/** Round toggles; at least one stays on. */
function DayPicker(props: { days: number[]; labels: (d: number) => string; value: number[]; onChange: (v: number[]) => void; ariaLabel: string; cols?: boolean }) {
  return (
    <div className={props.cols ? "grid w-fit grid-cols-7 gap-1" : "flex gap-1"} role="group" aria-label={props.ariaLabel}>
      {props.days.map((d) => {
        const on = props.value.includes(d);
        return (
          <button
            key={d}
            type="button"
            aria-pressed={on}
            onClick={() => {
              const next = on ? props.value.filter((x) => x !== d) : [...props.value, d].sort((a, b) => a - b);
              if (next.length > 0) props.onChange(next);
            }}
            className={`size-8 rounded-full text-sm tabular-nums transition-colors ${on ? "bg-primary text-primary-foreground" : "bg-muted text-muted-foreground hover:text-foreground"}`}
          >
            {props.labels(d)}
          </button>
        );
      })}
    </div>
  );
}

type Mode = "daily" | "weekly" | "monthly" | "yearly" | "every" | "after";
const MODES: Mode[] = ["daily", "weekly", "monthly", "yearly", "every", "after"];
const pad2 = (n: number) => String(n).padStart(2, "0");
/** Days in month m of a leap year (02-29 allowed; Rust uses 02-28 in other years). */
const monthDays = (m: number) => new Date(2000, m, 0).getDate();

/** 每年: 每 [n] 年的 [月] [日] → yearly:MM-DD or yearly:MM-DD/n. */
function YearlyEditor(props: { arg: string; onChange: (v: string) => void }) {
  const [md, every = "1"] = props.arg.split("/");
  const [m, d] = md.split("-").map(Number);
  const emit = (mm: number, dd: number, n: string) => props.onChange(`yearly:${pad2(mm)}-${pad2(Math.min(dd, monthDays(mm)))}${n === "1" ? "" : `/${n}`}`);
  return (
    <div className="flex flex-wrap items-center gap-2 text-sm">
      每 <NumberInput label="每几年" value={every} onChange={(v) => emit(m, d, v)} /> 年的
      <Dropdown ariaLabel="月" value={m} onChange={(v) => emit(v, d, every)} menuMaxHeight={260} options={Array.from({ length: 12 }, (_, i) => ({ value: i + 1, label: `${i + 1} 月` }))} />
      <Dropdown
        ariaLabel="日"
        value={d}
        onChange={(v) => emit(m, v, every)}
        menuMaxHeight={260}
        options={Array.from({ length: monthDays(m) }, (_, i) => ({ value: i + 1, label: `${i + 1} 日` }))}
      />
    </div>
  );
}

/** Repeat rule builder. `checkin` limits it to 每天 / 每周几天. */
function RuleEditor(props: { value: string; onChange: (v: string) => void; checkin?: boolean }) {
  const [kind, arg = ""] = props.value.split(":");
  const mode = (MODES.includes(kind as Mode) ? kind : "daily") as Mode;
  const list = mode === "weekly" || mode === "monthly" ? arg.split(",").filter(Boolean).map(Number) : [];
  const n = mode === "every" || mode === "after" ? arg.slice(0, -1) : "";
  const unit = (mode === "every" || mode === "after") && /[dwmy]$/.test(arg) ? arg.slice(-1) : "d";
  const modes: { value: Mode; label: string }[] = props.checkin
    ? [
        { value: "daily", label: "每天" },
        { value: "weekly", label: "每周几天" },
      ]
    : [
        { value: "daily", label: "每天" },
        { value: "weekly", label: "每周" },
        { value: "monthly", label: "每月" },
        { value: "yearly", label: "每年" },
        { value: "every", label: "每隔" },
        { value: "after", label: "完成后" },
      ];
  const today = parseYmd(localToday());
  const setMode = (m: Mode) =>
    props.onChange(
      {
        daily: "daily",
        weekly: `weekly:${((today.getDay() + 6) % 7) + 1}`,
        monthly: `monthly:${today.getDate()}`,
        yearly: `yearly:${pad2(today.getMonth() + 1)}-${pad2(today.getDate())}`,
        every: "every:2d",
        after: "after:7d",
      }[m],
    );
  return (
    <div className="space-y-2">
      <Segmented ariaLabel="重复方式" value={mode} onChange={setMode} options={modes} />
      {mode === "weekly" && (
        <DayPicker ariaLabel="星期几" days={[1, 2, 3, 4, 5, 6, 7]} labels={(d) => WEEK[d - 1]} value={list} onChange={(ds) => props.onChange(`weekly:${ds.join(",")}`)} />
      )}
      {mode === "monthly" && (
        <DayPicker
          cols
          ariaLabel="每月哪几天"
          days={Array.from({ length: 31 }, (_, i) => i + 1)}
          labels={String}
          value={list}
          onChange={(ds) => props.onChange(`monthly:${ds.join(",")}`)}
        />
      )}
      {mode === "yearly" && <YearlyEditor arg={arg} onChange={props.onChange} />}
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
        <div className="flex flex-wrap items-center gap-2 text-sm">
          完成后过 <NumberInput label="过多久" value={n} onChange={(v) => props.onChange(`after:${v}${unit}`)} />
          <Segmented
            ariaLabel="单位"
            value={unit}
            onChange={(u) => props.onChange(`after:${n}${u}`)}
            options={[
              { value: "d", label: "天" },
              { value: "w", label: "周" },
              { value: "m", label: "月" },
              { value: "y", label: "年" },
            ]}
          />
          再出现
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
          const can = inMonth && scheduled(d) && d <= today && (!t.startDate || d >= t.startDate) && (!t.dueDate || d <= t.dueDate);
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
        <DateInput ariaLabel="日期" value={day} max={today} onChange={setDay} />
        <Button onClick={() => m.log.mutate({ taskId: t.id, day, amount: Number(amount) || 1 })}>记上</Button>
      </div>
      <p className="text-xs text-muted-foreground">
        累计 {t.totalAmount} / {t.target} {unit}
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

/** 小拾 splits a saved 普通 task: suggestions to pick, 「加上」 adds them under it. */
/** 拆成子任务: the action (in 内容's label row) and, once 小拾 answered, its list (under 内容). */
function useSplitTask(taskId: number | null) {
  const status = usePickyStatus().data;
  const m = useTodoMutations();
  const toast = useToast();
  const [ideas, setIdeas] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const on = useAssistOn();
  if (!on || taskId == null) return { action: null, list: null };
  const split = async () => {
    setBusy(true);
    try {
      setIdeas(await assistApi.splitTask(taskId));
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  const list = ideas && (
    <IdeaList
      items={ideas}
      label={(t) => t}
      busy={m.addSubtasks.isPending}
      onAdd={(titles) =>
        m.addSubtasks.mutate(
          { parentId: taskId, titles },
            {
              onSuccess: () => {
                setIdeas(null);
                toast(`加上了 ${titles.length} 个子任务`);
              },
            },
          )
        }
      onCancel={() => setIdeas(null)}
    />
  );
  const action = !ideas && (
    <AssistAction disabled={busy} onClick={() => void split()}>
      {busy ? assistBusyText(status, "正在拆…") : "拆成子任务"}
    </AssistAction>
  );
  return { action, list };
}

/** 来自笔记: the note this task was made from; a click opens it in 念念不忘. */
function FromNote(props: { id: number }) {
  const navigate = useNavigate();
  const q = useQuery({ queryKey: ["notes", "list", { id: props.id }], queryFn: () => invoke<Note[]>("notes_list", { filter: { id: props.id } }) });
  const note = q.data?.[0];
  if (!note) return null;
  const open = () => void navigate("/notes", { state: { open: { module: "notes", kind: "note", id: note.id, title: "", detail: null, parent: null, day: null, amount: null, currency: null, flow: null } } });
  return (
    <Field label="来自笔记">
      <button type="button" onClick={open} className="flex min-w-0 items-center gap-1.5 text-left text-sm text-muted-foreground hover:text-foreground">
        <Feather size={13} className="shrink-0" style={{ color: "var(--module-notes)" }} />
        <span className="truncate">{excerpt(note.content)}</span>
      </button>
    </Field>
  );
}

type PanelProps = {
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
};

/** Opens once a saved task's attachments are in (they are part of its draft). */
export function TaskPanel(props: PanelProps) {
  const filesQ = useAttachmentsOf("task", props.task?.id ?? null);
  if (props.task && !filesQ.data) return null;
  return <TaskForm {...props} files={filesQ.data ?? []} />;
}

function TaskForm(props: PanelProps & { files: FileInfo[] }) {
  const { task } = props;
  const today = localToday();
  const initial = useMemo<Draft>(() => {
    const dated = task?.taskType !== "repeat";
    const dueDate = (dated ? task?.dueDate : null) ?? props.create?.dueDate ?? "";
    const startDate = (dated ? task?.startDate : null) ?? "";
    return {
      title: task?.title ?? props.create?.title ?? "",
      note: task?.note ?? props.create?.note ?? "",
      where: task ? whereOf(task) : (props.create?.where ?? ""),
      taskType: task?.taskType ?? "normal",
      ranged: !!(startDate || dueDate),
      startDate,
      dueDate,
      rule: task?.repeatRule ?? "daily",
      planDate: task?.planDate ?? "",
      repeatTotal: task?.repeatTotal?.toString() ?? "",
      doneCount: task?.taskType === "repeat" ? String(task.count) : "0",
      target: task?.target?.toString() ?? "",
      unit: task?.unit ?? "",
      link: task?.link ?? "",
      files: props.files,
    };
  }, [task, props.create, props.files]);
  const [d, setD] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const m = useTodoMutations();
  const toast = useToast();
  const isSub = task ? task.parentId !== null : props.create?.parentId !== undefined;
  const splitTask = useSplitTask(task && task.taskType === "normal" && d.taskType === "normal" ? task.id : null);

  // Counts can move underneath the panel (an occurrence finished from the list): follow them unless
  // the user already edited that field.
  const prevInitial = useRef(initial);
  useEffect(() => {
    const p = prevInitial.current;
    prevInitial.current = initial;
    setD((x) => {
      const next = { ...x };
      for (const k of ["doneCount", "startDate", "dueDate", "planDate"] as const) if (x[k] === p[k]) next[k] = initial[k];
      return next;
    });
  }, [initial]);

  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps

  const filled = useFilled<keyof Draft>();
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => {
    filled.clear(k);
    setD((x) => ({ ...x, [k]: v }));
  };
  // 一句话填好 (new tasks): 标题, the day or 时间段, and (not for a subtask) the group / goal
  const assistOn = useAssistOn();
  const quick = !task && assistOn;
  const fill = (f: QuickFillResult) => {
    const next = { ...d };
    const keys: (keyof Draft)[] = [];
    if (f.title) (next.title = f.title), keys.push("title");
    if (f.dueDate) (next.ranged = true), (next.dueDate = f.dueDate), (next.startDate = f.startDate ?? ""), keys.push("dueDate");
    if (props.create?.parentId === undefined && (f.goalId != null || f.groupId != null)) {
      const where = f.goalId != null ? `o:${f.goalId}` : `g:${f.groupId}`;
      if (whereOptions.some((o) => o.value === where)) (next.where = where), keys.push("where");
    }
    setD(next);
    filled.mark(keys);
  };
  /** 完成后 starts from a date: today unless one is set. */
  const withStart = (x: Draft): Draft => (x.taskType === "repeat" && !counted(x.rule) && !x.planDate ? { ...x, planDate: today } : x);
  const setType = (v: TaskType) =>
    setD((x) => withStart({ ...x, taskType: v, rule: v === "checkin" && !/^(daily|weekly)/.test(x.rule) ? "daily" : x.rule }));
  const setRule = (v: string) => setD((x) => withStart({ ...x, rule: v }));

  const whereOptions = props.groups.flatMap((g) => [
    { value: `g:${g.id}`, label: g.name },
    ...props.goals.filter((o) => o.groupId === g.id).map((o) => ({ value: `o:${o.id}`, label: `　${o.name}` })),
  ]);

  const num = (s: string) => (s.trim() ? Number(s) : null);
  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const type = d.taskType;
    const repeat = type === "repeat";
    const byCount = repeat && counted(d.rule);
    const fields: TaskPatch = {
      title: d.title,
      note: d.note.trim() || null,
      link: d.link.trim() || null,
      taskType: type,
      planDate: repeat && !byCount ? d.planDate || null : null,
      startDate: !repeat && d.ranged ? d.startDate || null : null,
      ...(repeat ? {} : { dueDate: d.ranged ? d.dueDate || null : null }), // a repeat's date comes from Rust
      repeatRule: repeat || type === "checkin" ? d.rule : null,
      repeatTotal: byCount ? num(d.repeatTotal) : null,
      repeatUntil: null,
      target: type === "progress" ? num(d.target) : null,
      unit: type === "progress" ? d.unit.trim() || null : null,
      ...(byCount ? { doneCount: num(d.doneCount) ?? 0 } : {}),
      attachments: d.files.map((x) => x.id),
    };
    try {
      if (!task) {
        const c = props.create!;
        await m.createTask.mutateAsync({ ...fields, ...(c.parentId !== undefined ? { parentId: c.parentId } : placeOf(d.where)), ...(c.fromNote !== undefined ? { fromNote: c.fromNote } : {}) });
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

  const saved = task !== null && task.taskType === d.taskType; // type records only for the saved type
  const range = (
    <Field label="时间">
      <div className="space-y-2">
        <Segmented
          ariaLabel="时间"
          value={d.ranged ? "range" : "none"}
          onChange={(v) => set("ranged", v === "range")}
          options={[
            { value: "none", label: "不设" },
            { value: "range", label: "时间段" },
          ]}
        />
        {d.ranged && (
          <div className={filled.halo("dueDate")}>
            <RangeInput start={d.startDate} end={d.dueDate} onStart={(v) => set("startDate", v)} onEnd={(v) => set("dueDate", v)} />
          </div>
        )}
      </div>
    </Field>
  );
  return (
    <SidePanel
      title={task ? "编辑任务" : isSub ? "添加子任务" : "新任务"}
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
      {quick && <QuickFill module="todo" onFill={fill} autoFocus />}
      <Field label="标题">
        <TextInput autoFocus={!quick} value={d.title} onChange={(e) => set("title", e.target.value)} placeholder="必填" maxLength={500} className={filled.cls("title")} />
      </Field>
      <Field label="内容" action={splitTask.action}>
        <Polish module="todo" value={d.note} onChange={(v) => set("note", v)}>
          <TextArea rows={4} value={d.note} onChange={(e) => set("note", e.target.value)} maxLength={5000} />
        </Polish>
        {splitTask.list}
      </Field>
      {task?.fromNote != null && <FromNote id={task.fromNote} />}
      <Field label="类型">
        {props.hasChildren ? (
          <p className="text-sm">普通</p>
        ) : (
          <Segmented ariaLabel="类型" value={d.taskType} onChange={setType} options={(Object.keys(TASK_TYPES) as TaskType[]).map((k) => ({ value: k, label: TASK_TYPES[k] }))} />
        )}
      </Field>
      <Field label="位置">
        {isSub ? (
          <p className="text-sm text-muted-foreground">在「{(props.parentTitle ?? props.create?.parentTitle ?? "").replace(/\*\*/g, "")}」下面</p>
        ) : (
          <div className={filled.halo("where")}>
            <Dropdown ariaLabel="位置" value={d.where} onChange={(v) => set("where", v)} options={whereOptions} placeholder="选择分组或目标" />
          </div>
        )}
      </Field>

      {d.taskType === "normal" && range}
      {d.taskType === "repeat" && (
        <>
          <Field label="重复">
            <RuleEditor value={d.rule} onChange={setRule} />
          </Field>
          {counted(d.rule) && (
            <div className="flex gap-4">
              <Field label="一共几次">
                <NumberInput label="一共几次" value={d.repeatTotal} onChange={(v) => set("repeatTotal", v)} placeholder="不限" />
              </Field>
              <Field label="已经做了几次">
                <NumberInput label="已经做了几次" min={0} value={d.doneCount} onChange={(v) => set("doneCount", v)} />
              </Field>
            </div>
          )}
          {ruleKind(d.rule) === "after" && (
            <Field label="开始日期">
              <div>
                <DateInput clearable ariaLabel="开始日期" value={d.planDate} onChange={(v) => set("planDate", v)} />
              </div>
            </Field>
          )}
          {saved && task.dueDate && d.rule === task.repeatRule && (
            <div className="flex items-center justify-between gap-2 rounded-card border px-3 py-2 text-sm">
              {task.done ? (
                <span>已全部完成，共 {task.count} 次</span>
              ) : (
                <>
                  <span className="tabular-nums">
                    第 {task.count + 1} 次：{slashDate(task.dueDate)}
                    {task.lastDate && task.lastDate !== task.dueDate && <span className="block text-muted-foreground">最后一次：{slashDate(task.lastDate)}</span>}
                  </span>
                  <Button
                    onClick={() =>
                      // the date moves on in Rust: close so a stale draft can't save over it
                      m.skip.mutate(task.id, {
                        onSuccess: () => {
                          toast("已跳过这一次");
                          props.onDone();
                        },
                      })
                    }
                  >
                    跳过这一次
                  </Button>
                </>
              )}
            </div>
          )}
        </>
      )}
      {d.taskType === "checkin" && (
        <>
          <Field label="哪些天打卡">
            <RuleEditor checkin value={d.rule} onChange={setRule} />
          </Field>
          {range}
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
              <TextInput value={d.unit} onChange={(e) => set("unit", e.target.value)} maxLength={10} className="w-28" />
            </Field>
          </div>
          {range}
          {saved && (
            <Field label="记一笔">
              <ProgressLog task={task} />
            </Field>
          )}
        </>
      )}
      <Field label="链接">
        <TextInput value={d.link} onChange={(e) => set("link", e.target.value)} maxLength={1000} />
      </Field>
      <AttachmentField files={d.files} onChange={(v) => set("files", v)} />
    </SidePanel>
  );
}
