// 定时 (its own dialog beside 管理 and 插件库 at the bottom of the conversation list; rules in Rust schedule.rs): the list on the left (今日简报 first, then the tasks:
// name, when, off / running), the chosen one on the right: 名称, 什么时候, 模型 and 思考, 要做什么, saved with
// 保存 (one undoable step); 打开 switch, 立即运行, 删除, and 下次 / 上次 (opens the conversation it wrote last).
// 今日简报 (brief.rs) only has 打开, 模型 and 思考, 重写今天的. With 思考 on only models that can think are
// listed, off only ones that can do without; a chosen one that doesn't fit goes back to 小拾现在选的.
import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Plus, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { formatDay, toYmd, weekday } from "@/shared/dates";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button, IconButton, Segmented, Switch, TextArea, TextInput } from "@/shared/ui";
import { BRIEF_KEY, LIBRARY_KEY, SCHEDULES_KEY, pickyApi, ruleText, useBrief, usePickyStatus, useSchedules, type Schedule, type ScheduleInput } from "./api";
import { thinkFits, thinkOf } from "./Thinking";

type Confirm = ReturnType<typeof useConfirm>[0];

const BLANK: ScheduleInput = { name: "", prompt: "", every: "day", day: null, month: null, at: "09:00", model: null, thinking: true };
const DAYS = (day: number | null) => [...Array.from({ length: 28 }, (_, i) => ({ value: i + 1, label: `${i + 1} 号` })), ...(day && day > 28 && day < 31 ? [{ value: day, label: `${day} 号` }] : []), { value: 31, label: "最后一天" }];

/** The model menu: 小拾现在选的, then the local and cloud models that fit 思考 (one gone stays, marked). */
export function useModels(current: string | null, thinking: boolean) {
  const status = usePickyStatus().data;
  const fits = (code: string) => thinkFits(thinkOf(status, code), thinking);
  const models = [
    { value: null as string | null, label: "小拾现在选的" },
    ...(status?.local.models ?? []).map((m) => ({ value: `local:${m.key}` as string | null, label: m.name, group: "本地" })),
    ...(status?.cloud ?? []).map((m) => ({ value: `cloud:${m.id}` as string | null, label: m.name, group: "云端" })),
  ].filter((m) => m.value == null || m.value === current || fits(m.value));
  if (current && !models.some((m) => m.value === current)) models.push({ value: current, label: "（已经不在了）", group: "" });
  /** The model to keep when 思考 changes (one that no longer fits → 小拾现在选的). */
  const keep = (model: string | null, on: boolean) => (model && !thinkFits(thinkOf(status, model), on) ? null : model);
  return { models, keep };
}
const WEEKDAYS = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

/** "2026-10-19 09:00" → 「10 月 19 日 周一 09:00」 */
function slotText(slot: string): string {
  const [d, t] = slot.split(" ");
  return `${formatDay(d)} ${weekday(d)} ${t}`;
}

/** A stored UTC time → local 「10 月 12 日 09:00」 */
function localText(iso: string): string {
  const d = new Date(iso);
  return `${formatDay(toYmd(d))} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function SchedulesDialog(props: { onOpenSession: (id: number) => void; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [confirm, confirmDialog] = useConfirm();
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  return (
    <dialog
      ref={ref}
      aria-label="定时"
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className="m-auto flex h-[78vh] max-h-[720px] w-[860px] max-w-[92vw] flex-col rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <div className="flex items-center gap-4 border-b px-5 py-3">
        <p className="text-base font-bold">定时</p>
        <div className="flex-1" />
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      <div className="flex min-h-0 flex-1">
        <SchedulesTab
          confirm={confirm}
          onOpenSession={(id) => {
            props.onClose();
            props.onOpenSession(id);
          }}
        />
      </div>
      {confirmDialog}
    </dialog>
  );
}

function SchedulesTab(props: { confirm: Confirm; onOpenSession: (id: number) => void }) {
  const list = useSchedules().data ?? [];
  const brief = useBrief().data;
  const [selected, setSelected] = useState<number | "new" | "brief">("brief");
  const current = typeof selected === "number" ? list.find((s) => s.id === selected) : undefined;
  return (
    <>
      <div className="flex w-56 shrink-0 flex-col border-r">
        <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1 px-3 py-3">
          <button
            type="button"
            onClick={() => setSelected("brief")}
            className={`block w-full rounded-control px-2 py-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring ${selected === "brief" ? "bg-accent" : "hover:bg-muted/70"}`}
          >
            <span className={`block truncate text-sm ${brief?.enabled === false ? "text-muted-foreground" : ""}`}>今日简报</span>
            <span className="block truncate text-xs text-muted-foreground">{brief?.enabled === false ? "已关闭" : "每天第一次打开首页时"}</span>
          </button>
          {list.map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => setSelected(s.id)}
              className={`block w-full rounded-control px-2 py-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring ${current?.id === s.id ? "bg-accent" : "hover:bg-muted/70"}`}
            >
              <span className={`block truncate text-sm ${s.enabled ? "" : "text-muted-foreground"}`}>{s.name}</span>
              <span className="block truncate text-xs text-muted-foreground">{s.running ? "正在运行…" : s.enabled ? ruleText(s) : "已关闭"}</span>
            </button>
          ))}
        </div>
        <div className="border-t p-2">
          <button type="button" onClick={() => setSelected("new")} className="flex items-center gap-1.5 rounded-control px-2 py-1.5 text-sm text-muted-foreground hover:bg-muted hover:text-foreground">
            <Plus size={14} /> 新建定时任务
          </button>
        </div>
      </div>
      <div className="min-w-0 flex-1">
        {selected === "brief" ? (
          <BriefEditor />
        ) : selected === "new" ? (
          <ScheduleEditor key="new" schedule={null} confirm={props.confirm} onSaved={(id) => setSelected(id ?? "brief")} onOpenSession={props.onOpenSession} />
        ) : (
          current && <ScheduleEditor key={current.id} schedule={current} confirm={props.confirm} onSaved={(id) => setSelected(id ?? "brief")} onOpenSession={props.onOpenSession} />
        )}
      </div>
    </>
  );
}

function BriefEditor() {
  const qc = useQueryClient();
  const toast = useToast();
  const b = useBrief().data;
  const { models, keep } = useModels(b?.model ?? null, b?.thinking ?? true);
  if (!b) return null;
  const put = (enabled: boolean, model: string | null, thinking = b.thinking) =>
    void pickyApi.briefSet(enabled, model, thinking).then(
      (v) => qc.setQueryData(BRIEF_KEY, v),
      (e) => toast(errorText(e), "error"),
    );
  const rewrite = () =>
    void pickyApi.briefWrite(true).then(
      (v) => (qc.setQueryData(BRIEF_KEY, v), toast("开始重写今天的简报了，写好后在首页")),
      (e) => toast(errorText(e), "error"),
    );
  return (
    <div className="flex h-full flex-col gap-4 p-5">
      <div className="flex items-end gap-3">
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">名称</span>
          <p className="py-1.5 text-sm">今日简报</p>
        </div>
        <div className="flex items-center gap-2 pb-2 text-sm">
          打开 <Switch label="打开" checked={b.enabled} onChange={(on) => put(on, b.model)} />
        </div>
      </div>
      <div className="flex flex-col gap-1.5">
        <span className="text-xs text-muted-foreground">什么时候</span>
        <p className="text-sm">每天第一次打开首页时写，当天不再变</p>
      </div>
      <div className="flex flex-col gap-1.5">
        <span className="text-xs text-muted-foreground">模型</span>
        <div className="flex items-center gap-4">
          <Dropdown<string | null> ariaLabel="模型" value={b.model} options={models} onChange={(m) => put(b.enabled, m)} />
          <span className="flex items-center gap-2 text-sm">
            思考 <Switch label="思考" checked={b.thinking} onChange={(on) => put(b.enabled, keep(b.model, on), on)} />
          </span>
        </div>
      </div>
      <div className="flex flex-col gap-1.5">
        <span className="text-xs text-muted-foreground">写什么</span>
        <p className="text-sm leading-relaxed">今天的待办、快到期的和过期的；该跟进的投递、在笔试面试的；最近的笔记里常在想什么。</p>
      </div>
      <div className="flex-1" />
      <div className="flex items-center gap-2">
        <Button onClick={rewrite} disabled={!b.enabled || b.state === "writing"}>
          重写今天的
        </Button>
      </div>
    </div>
  );
}

function ScheduleEditor(props: { schedule: Schedule | null; confirm: Confirm; onSaved: (id: number | null) => void; onOpenSession: (id: number) => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const s = props.schedule;
  const initial: ScheduleInput = s ? { name: s.name, prompt: s.prompt, every: s.every, day: s.day, month: s.month ?? null, at: s.at, model: s.model, thinking: s.thinking } : BLANK;
  const [form, setForm] = useState<ScheduleInput>(initial);
  const set = (p: Partial<ScheduleInput>) => setForm((f) => ({ ...f, ...p }));
  const dirty = JSON.stringify(form) !== JSON.stringify(initial);
  const refresh = () => qc.invalidateQueries({ queryKey: SCHEDULES_KEY });
  const fail = (e: unknown) => toast(errorText(e), "error");

  const save = async () => {
    try {
      const id = await pickyApi.scheduleSave(s?.id ?? null, form);
      await refresh();
      props.onSaved(id);
    } catch (e) {
      fail(e);
    }
  };
  const remove = async () => {
    if (!s) return;
    if (!(await props.confirm({ title: `删除定时任务「${s.name}」？`, message: "它写过的对话还在「定时」里。", confirmLabel: "删除", danger: true }))) return;
    try {
      await pickyApi.scheduleDelete(s.id);
      props.onSaved(null);
      await refresh();
    } catch (e) {
      fail(e);
    }
  };
  const run = async () => {
    if (!s) return;
    try {
      await pickyApi.scheduleRun(s.id);
      toast(`「${s.name}」开始运行了，写好后在「定时」里`);
      await refresh();
      void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
    } catch (e) {
      fail(e);
    }
  };
  const enable = (on: boolean) =>
    void pickyApi.scheduleEnable(s!.id, on).then(refresh, fail);

  const { models, keep } = useModels(form.model, form.thinking);

  return (
    <div className="flex h-full flex-col gap-4 p-5">
      <div className="flex items-end gap-3">
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">名称</span>
          <TextInput aria-label="名称" value={form.name} maxLength={30} onChange={(e) => set({ name: e.target.value })} />
        </div>
        {s && (
          <div className="flex items-center gap-2 pb-2 text-sm">
            打开 <Switch label="打开" checked={s.enabled} onChange={enable} />
          </div>
        )}
      </div>
      <div className="flex flex-col gap-1.5">
        <span className="text-xs text-muted-foreground">什么时候</span>
        <div className="flex flex-wrap items-center gap-2">
          <Segmented<ScheduleInput["every"]>
            value={form.every}
            onChange={(every) =>
              set({
                every,
                day: every === "day" ? null : every === form.every ? form.day : 1,
                month: every === "year" ? (form.month ?? 1) : null,
              })
            }
            options={[
              { value: "day", label: "每天" },
              { value: "week", label: "每周" },
              { value: "month", label: "每月" },
              { value: "year", label: "每年" },
            ]}
          />
          {form.every === "week" && <Dropdown<number> ariaLabel="星期几" value={form.day ?? 1} options={WEEKDAYS.map((w, i) => ({ value: i + 1, label: w }))} onChange={(day) => set({ day })} />}
          {form.every === "year" && (
            <Dropdown<number> ariaLabel="几月" value={form.month ?? 1} menuMaxHeight={280} options={Array.from({ length: 12 }, (_, i) => ({ value: i + 1, label: `${i + 1} 月` }))} onChange={(month) => set({ month })} />
          )}
          {(form.every === "month" || form.every === "year") && (
            <Dropdown<number> ariaLabel="几号" value={form.day ?? 1} menuMaxHeight={280} options={DAYS(form.day)} onChange={(day) => set({ day })} />
          )}
          <TextInput aria-label="几点" value={form.at} maxLength={5} inputMode="numeric" onChange={(e) => set({ at: e.target.value })} className="max-w-20 text-center tabular-nums" />
        </div>
      </div>
      <div className="flex flex-col gap-1.5">
        <span className="text-xs text-muted-foreground">模型</span>
        <div className="flex items-center gap-4">
          <Dropdown<string | null> ariaLabel="模型" value={form.model} options={models} onChange={(model) => set({ model })} />
          <span className="flex items-center gap-2 text-sm">
            思考 <Switch label="思考" checked={form.thinking} onChange={(thinking) => set({ thinking, model: keep(form.model, thinking) })} />
          </span>
        </div>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-1.5">
        <span className="text-xs text-muted-foreground">要做什么</span>
        <TextArea aria-label="要做什么" value={form.prompt} onChange={(e) => set({ prompt: e.target.value })} className="min-h-0 flex-1 resize-none font-reading leading-relaxed" />
        {s?.builtin === "yearly" && (
          <p className="text-xs text-muted-foreground">读这一年的笔记、待办和打卡、画画、投递、出行、书影和跟小拾的对话（私密的不读）：模型放得下就一次读完，放不下先按月整理要点；钱来钱往不读。</p>
        )}
      </div>
      {s && (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
          {s.running ? <span>正在运行…</span> : s.next && <span>下次 {slotText(s.next)}</span>}
          {s.lastSession != null && s.lastAt && (
            <button type="button" onClick={() => props.onOpenSession(s.lastSession!)} className="rounded-control underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring">
              上次 {localText(s.lastAt)}
            </button>
          )}
        </div>
      )}
      <div className="flex items-center gap-2">
        {s && (
          <>
            <Button variant="ghost" className="text-destructive" onClick={() => void remove()}>
              删除…
            </Button>
            <Button onClick={() => void run()} disabled={s.running || dirty}>
              立即运行
            </Button>
          </>
        )}
        <div className="flex-1" />
        {s && dirty && <Button onClick={() => setForm(initial)}>还原</Button>}
        <Button variant="primary" disabled={!dirty || !form.name.trim() || !form.prompt.trim()} onClick={() => void save()}>
          保存
        </Button>
      </div>
    </div>
  );
}
