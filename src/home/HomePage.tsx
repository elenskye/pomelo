// 首页 (simple version, fixed layout): greeting + date, 年度总结 in the new year's first week (YearCard.tsx), 今日简报
// (BriefCard.tsx), 今年的数字, then two columns — left 今天的待办, 快到的日子 (only when there are some) and
// 最近记录, right 总资产, 求职近况, 在读在看, 绘画进度. Hidden modules' blocks are left out. Rows open the
// record in its module (same hand-off as 全局搜索); a block's title opens the module.
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useNavigate } from "react-router";
import { ChevronRight, type LucideIcon } from "lucide-react";
import { useHiddenModules } from "@/app/hidden";
import { moduleById } from "@/app/modules";
import { countText, type Day } from "@/modules/days/api";
import { money, moneyShort, dayTick, dayTitle, type Currency } from "@/modules/finance/money";
import { STAGE_STYLE } from "@/modules/jobs/api";
import { useAgenda, type Task } from "@/modules/todo/api";
import { PlacedList } from "@/modules/todo/TaskList";
import { LineChart } from "@/shared/charts";
import { formatDay, toYmd, weekday } from "@/shared/dates";
import type { Hit } from "@/shared/open";
import { PageHeader, ProgressBar } from "@/shared/ui";
import { useOverview, type CourseLine, type JobsCard, type Point, type Recent, type Year } from "./api";
import { BriefCard } from "./BriefCard";
import { YearCard } from "./YearCard";

/** Local clock, re-read every minute and when the window comes back. */
function useNow() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const tick = () => setNow(new Date());
    const timer = setInterval(tick, 60_000);
    window.addEventListener("focus", tick);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", tick);
    };
  }, []);
  return now;
}

function greeting(h: number) {
  if (h >= 5 && h < 11) return "早上好";
  if (h >= 11 && h < 13) return "中午好";
  if (h >= 13 && h < 18) return "下午好";
  if (h >= 18 && h < 23) return "晚上好";
  return "夜深了";
}

const pad = (n: number) => String(n).padStart(2, "0");

/** When a 最近 row happened: 14:30 today, 昨天, 10 / 06 this year, else 2025 / 10 / 06. */
function when(at: string, today: string) {
  const d = new Date(at);
  const day = toYmd(d);
  if (day === today) return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  const y = new Date(`${today}T00:00:00`);
  y.setDate(y.getDate() - 1);
  if (day === toYmd(y)) return "昨天";
  return day.slice(0, 4) === today.slice(0, 4) ? `${day.slice(5, 7)} / ${day.slice(8)}` : day.split("-").join(" / ");
}

const taskHit = (t: Task): Hit => ({
  module: "todo",
  kind: "task",
  id: t.id,
  title: t.title,
  detail: null,
  parent: t.goalId !== null ? -t.goalId : t.groupId,
  day: null,
  amount: null,
  currency: null,
  flow: null,
});

const BLANK: Hit = { module: "", kind: "", id: 0, title: "", detail: null, parent: null, day: null, amount: null, currency: null, flow: null };

/** record: open one record (as 全局搜索 does); module: go to a module, optionally with a view to open (kind). */
function useOpen() {
  const navigate = useNavigate();
  return {
    record: (h: Hit) => void navigate(`/${h.module}`, { state: { open: h } }),
    module: (id: string, kind?: string) => void navigate(`/${id}`, kind ? { state: { open: { ...BLANK, module: id, kind } } } : undefined),
  };
}

/** A titled block; the title (icons + name) runs onTitle (opens the module). */
function Block(props: { title: string; icons: { icon: LucideIcon; color: string }[]; onTitle: () => void; right?: ReactNode; children: ReactNode }) {
  return (
    <section>
      <div className="mb-1.5 flex items-center justify-between gap-3 px-1">
        <button
          type="button"
          onClick={props.onTitle}
          className="group/t flex items-center gap-1.5 rounded-control text-sm font-bold outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {props.icons.map(({ icon: Icon, color }, i) => (
            <Icon key={i} size={15} strokeWidth={1.75} style={{ color }} />
          ))}
          {props.title}
          <ChevronRight size={14} className="text-muted-foreground opacity-0 transition-opacity duration-150 group-hover/t:opacity-100 group-focus-visible/t:opacity-100" />
        </button>
        {props.right && <div className="text-xs text-muted-foreground tabular-nums">{props.right}</div>}
      </div>
      {props.children}
    </section>
  );
}

const box = "overflow-hidden rounded-card border bg-card";
const rowClass = "flex w-full items-baseline gap-3 border-b px-3 py-2 text-left text-sm transition-colors duration-150 outline-none last:border-b-0 hover:bg-muted/70 focus-visible:bg-muted/70";

function Empty(props: { children: ReactNode }) {
  return <p className={`${box} px-4 py-6 text-center text-sm text-muted-foreground`}>{props.children}</p>;
}

function icon(id: string) {
  const m = moduleById(id);
  return { icon: m.icon, color: m.color };
}

// ---------- blocks ----------

function YearRow(props: { year: Year; go: (id: string) => void }) {
  const y = props.year;
  const all: [string, string, number | null, string][] = [
    ["books", "读完", y.books, "本"],
    ["films", "看完", y.films, "部"],
    ["drawing", "画完", y.lessons, "节"],
    ["todo", "完成", y.todos, "件"],
    ["notes", "笔记", y.notes, "条"],
    ["jobs", "投出", y.jobs, "份"],
  ];
  const cells = all.filter((c): c is [string, string, number, string] => c[2] !== null);
  if (cells.length === 0) return null;
  return (
    <section>
      <h2 className="mb-1.5 px-1 text-sm font-bold">{Number(y.year)} 年</h2>
      <div className={`${box} grid divide-x`} style={{ gridTemplateColumns: `repeat(${cells.length}, minmax(0, 1fr))` }}>
        {cells.map(([id, label, n, unit]) => {
          const { icon: Icon, color } = icon(id);
          return (
            <button
              key={id}
              type="button"
              onClick={() => props.go(id)}
              className="flex flex-col gap-0.5 px-4 py-3 text-left transition-colors duration-150 outline-none hover:bg-muted/70 focus-visible:bg-muted/70"
            >
              <span className="flex items-center gap-1 text-xs text-muted-foreground">
                <Icon size={12} strokeWidth={1.75} style={{ color }} />
                {label}
              </span>
              <span className="whitespace-nowrap">
                <b className="text-xl tabular-nums">{n}</b>
                <span className="ml-1 text-xs text-muted-foreground">{unit}</span>
              </span>
            </button>
          );
        })}
      </div>
    </section>
  );
}

function TodayBlock(props: { today: string; onOpen: (t: Task) => void; go: () => void }) {
  const { data: agenda } = useAgenda(props.today);
  if (!agenda) return null;
  const items = [...agenda.overdue, ...agenda.due];
  return (
    <Block
      title="今天的待办"
      icons={[icon("todo")]}
      onTitle={props.go}
      right={
        <>
          {agenda.overdue.length > 0 && <span className="text-destructive">已过期 {agenda.overdue.length}</span>}
          {agenda.overdue.length > 0 && agenda.doneToday.length > 0 && " · "}
          {agenda.doneToday.length > 0 && `完成了 ${agenda.doneToday.length} 件`}
        </>
      }
    >
      {items.length > 0 ? (
        <PlacedList items={items} today={props.today} selectedId={null} onOpen={props.onOpen} />
      ) : (
        <Empty>今天没有安排，喝杯茶吧 🍵</Empty>
      )}
    </Block>
  );
}

const dayHit = (d: Day): Hit => ({ ...BLANK, module: "days", kind: "day", id: d.id, title: d.name });

function DaysBlock(props: { days: Day[]; open: (h: Hit) => void; go: () => void }) {
  if (props.days.length === 0) return null;
  return (
    <Block title="快到的日子" icons={[icon("days")]} onTitle={props.go}>
      <div className={box}>
        {props.days.map((d) => {
          const [before, n, after] = countText(d);
          const tone = d.days === 0 ? "text-primary" : d.days < 0 ? "text-destructive" : "text-[var(--status-doing-fg)]";
          return (
            <button key={d.id} type="button" onClick={() => props.open(dayHit(d))} className={rowClass}>
              <span className="min-w-0 flex-1 truncate">{d.name}</span>
              <span className={`shrink-0 text-xs tabular-nums ${tone}`}>
                {before}
                {n !== null && ` ${n} ${after}`}
              </span>
            </button>
          );
        })}
      </div>
    </Block>
  );
}

function recentDetail(r: Recent) {
  if (r.kind === "entry" && r.amount !== null && r.currency) return [money(r.amount, r.currency as Currency, r.flow === "in"), r.detail].filter(Boolean).join(" · ");
  if (r.kind === "task") return `完成 · ${r.detail}`;
  return r.detail;
}

function RecentBlock(props: { items: Recent[]; today: string; open: (h: Hit) => void }) {
  if (props.items.length === 0) return null;
  return (
    <section>
      <h2 className="mb-1.5 px-1 text-sm font-bold">最近记录</h2>
      <div className={box}>
        {props.items.map((r) => {
          const { icon: Icon, color } = icon(r.module);
          const d = recentDetail(r);
          return (
            <button key={`${r.kind}${r.id}`} type="button" onClick={() => props.open(r)} className={rowClass}>
              <Icon size={14} strokeWidth={1.75} style={{ color }} className="shrink-0 self-center" />
              <span className="min-w-0 truncate">{r.title}</span>
              {d && <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground tabular-nums">{d}</span>}
              <span className="ml-auto shrink-0 text-xs text-muted-foreground tabular-nums">{when(r.at, props.today)}</span>
            </button>
          );
        })}
      </div>
    </section>
  );
}

function AssetsBlock(props: { points: Point[]; go: () => void }) {
  const p = props.points;
  const last = p[p.length - 1];
  const prev = p[p.length - 2];
  const delta = last && prev ? last.total - prev.total : null;
  return (
    <Block title="总资产" icons={[icon("finance")]} onTitle={props.go} right={last && formatDay(last.day)}>
      {!last ? (
        <Empty>还没更新过余额</Empty>
      ) : (
        <div className={`${box} px-4 py-3`}>
          <p className="text-xl font-bold tabular-nums">{money(last.total, "CNY")}</p>
          {delta !== null && (
            <p className={`text-xs tabular-nums ${delta >= 0 ? "text-[var(--status-done-fg)]" : "text-destructive"}`}>
              比上次 {delta >= 0 ? "+" : "−"}
              {money(Math.abs(delta), "CNY")}
            </p>
          )}
          {p.length > 1 && (
            <div className="pt-2">
              <LineChart
                name="总资产（人民币）"
                baseline="auto"
                height={130}
                // first, middle and last day only: the block is narrow
                labels={p.map((x, i) => (i === 0 || i === p.length - 1 || i === Math.round((p.length - 1) / 2) ? dayTick(x.day, p[i - 1]?.day) : ""))}
                values={p.map((x) => x.total)}
                format={(n) => money(n, "CNY")}
                axisFormat={(n) => moneyShort(n, "CNY")}
                title={(i) => dayTitle(p[i].day)}
              />
            </div>
          )}
        </div>
      )}
    </Block>
  );
}

const LIST = 6;

function JobsBlock(props: { card: JobsCard; open: (h: Hit) => void; go: () => void }) {
  const c = props.card;
  const lines = [...c.follow, ...c.active];
  const stat = (label: string, n: number, unit = "") => (
    <div className="flex flex-col px-3 py-2">
      <span className="text-xs text-muted-foreground">{label}</span>
      <b className="tabular-nums">
        {n}
        {unit && <span className="ml-0.5 text-xs font-normal text-muted-foreground">{unit}</span>}
      </b>
    </div>
  );
  return (
    <Block title="求职近况" icons={[icon("jobs")]} onTitle={props.go} right={c.total > 0 && `共 ${c.total} 条`}>
      {c.total === 0 ? (
        <Empty>还没有投递</Empty>
      ) : (
        <div className={box}>
          <div className="grid grid-cols-3 divide-x border-b">
            {stat("本周投了", c.thisWeek, "份")}
            {stat("进行中", c.ongoing)}
            {stat("拿到", c.got)}
          </div>
          {lines.slice(0, LIST).map((j) => (
            <button key={j.id} type="button" onClick={() => props.open(j)} className={rowClass}>
              <span className="min-w-0 flex-1 truncate">{j.title}</span>
              {j.stage === "applied" ? (
                <span className="shrink-0 text-xs text-[var(--status-doing-fg)] tabular-nums">该跟进 · 等了 {j.waitingDays} 天</span>
              ) : (
                <span className="shrink-0 rounded px-1.5 text-xs" style={{ background: STAGE_STYLE[j.stage].bg, color: STAGE_STYLE[j.stage].fg }}>
                  {j.status}
                </span>
              )}
            </button>
          ))}
          {lines.length > LIST && (
            <button type="button" onClick={props.go} className={`${rowClass} text-xs text-muted-foreground`}>
              还有 {lines.length - LIST} 条
            </button>
          )}
        </div>
      )}
    </Block>
  );
}

function ReadingBlock(props: { items: Hit[]; hidden: string[]; open: (h: Hit) => void; go: () => void }) {
  const icons = (["books", "films"] as const).filter((m) => !props.hidden.includes(m)).map(icon);
  const title = props.hidden.includes("films") ? "在读" : props.hidden.includes("books") ? "在看" : "在读在看";
  return (
    <Block title={title} icons={icons} onTitle={props.go}>
      {props.items.length === 0 ? (
        <Empty>这会儿没有{title}的</Empty>
      ) : (
        <div className={box}>
          {props.items.map((h) => {
            const { icon: Icon, color } = icon(h.module);
            return (
              <button key={`${h.kind}${h.id}`} type="button" onClick={() => props.open(h)} className={rowClass}>
                <Icon size={14} strokeWidth={1.75} style={{ color }} className="shrink-0 self-center" />
                <span className="min-w-0 truncate">{h.title}</span>
                {h.detail && <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{h.detail}</span>}
              </button>
            );
          })}
        </div>
      )}
    </Block>
  );
}

function DrawingBlock(props: { courses: CourseLine[]; open: (h: Hit) => void; go: () => void }) {
  return (
    <Block title="绘画进度" icons={[icon("drawing")]} onTitle={props.go}>
      {props.courses.length === 0 ? (
        <Empty>没有在学的课</Empty>
      ) : (
        <div className={box}>
          {props.courses.map((c) => (
            <button
              key={c.id}
              type="button"
              onClick={() => (c.next ? props.open(c.next) : props.go())}
              className="block w-full space-y-1.5 border-b px-3 py-2.5 text-left text-sm transition-colors duration-150 outline-none last:border-b-0 hover:bg-muted/70 focus-visible:bg-muted/70"
            >
              <span className="flex items-baseline justify-between gap-2">
                <span className="truncate">{c.name}</span>
                <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
                  {c.done} / {c.total}
                </span>
              </span>
              <ProgressBar done={c.done} doing={c.doing} total={c.total} />
              {c.next && <span className="block truncate text-xs text-muted-foreground">下一节 {c.next.title}</span>}
            </button>
          ))}
        </div>
      )}
    </Block>
  );
}

export default function HomePage() {
  const now = useNow();
  const today = toYmd(now);
  const [hidden] = useHiddenModules();
  const { data, refetch } = useOverview(hidden);
  const agenda = useAgenda(today);
  const open = useOpen();
  const shown = (id: string) => !hidden.includes(id);
  // ticking a task here changes 最近 and 今年 too
  const seen = useRef(agenda.dataUpdatedAt);
  useEffect(() => {
    if (agenda.dataUpdatedAt === seen.current) return;
    seen.current = agenda.dataUpdatedAt;
    void refetch();
  }, [agenda.dataUpdatedAt, refetch]);

  const main = [
    shown("todo") && <TodayBlock key="todo" today={today} onOpen={(t) => open.record(taskHit(t))} go={() => open.module("todo")} />,
    data?.days && <DaysBlock key="days" days={data.days} open={open.record} go={() => open.module("days")} />,
    data && <RecentBlock key="recent" items={data.recent} today={today} open={open.record} />,
  ];
  const side = data
    ? [
        data.assets && <AssetsBlock key="assets" points={data.assets} go={() => open.module("finance", "assets")} />,
        data.jobs && <JobsBlock key="jobs" card={data.jobs} open={open.record} go={() => open.module("jobs")} />,
        data.reading && <ReadingBlock key="reading" items={data.reading} hidden={hidden} open={open.record} go={() => open.module(shown("books") ? "books" : "films")} />,
        data.drawing && <DrawingBlock key="drawing" courses={data.drawing} open={open.record} go={() => open.module("drawing")} />,
      ]
    : [];

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader title={greeting(now.getHours())} right={<p className="pointer-events-none text-sm text-muted-foreground">{formatDay(today)} {weekday(today)}</p>} />
      <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
        <div className="space-y-5 px-8 pb-6">
          {shown("picky") && <YearCard />}
          {shown("picky") && <BriefCard today={today} onTitle={() => open.module("picky")} />}
          {data && <YearRow year={data.year} go={(id) => open.module(id)} />}
          <div className="grid grid-cols-1 items-start gap-5 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
            <div className="space-y-5">{main}</div>
            <div className="space-y-5">{side}</div>
          </div>
        </div>
      </div>
    </div>
  );
}

