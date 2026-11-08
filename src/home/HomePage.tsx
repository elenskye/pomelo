// 首页完整版 (docs/requirements.md「首页」, design.md「首页」; visual-atelier's canvases/pomelo-pages Desk / DeskRight):
// a desk twice as wide as the window (Desk.tsx: slide it left and right, throw the notes around, where they rest is
// kept). Left half as the B3 desk: the greeting, 今日简报 (with 年度总结 above it in the new year's first week),
// 今天的待办, 快到的日子, 在读在看, the 资产小票, 念念不忘一条, 今年的数字; right half: 本月小月历 (opens 日历),
// 今年按月, 求职近况, 绘画进度, 最近记录, 那年今日, 一网打尽. Hidden modules' notes are left out. Rows open the
// record in its module (same hand-off as 全局搜索); a note's title opens the module.
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { useHiddenModules } from "@/app/hidden";
import { useTabs } from "@/app/tabs";
import { type Day } from "@/modules/days/api";
import { useAgenda, type Task } from "@/modules/todo/api";
import { toYmd } from "@/shared/dates";
import type { Hit } from "@/shared/open";
import { useOverview } from "./api";
import { BriefCard } from "./BriefCard";
import { Desk, DeskNote } from "./Desk";
import { AssetsNote, DaysNote, DrawingNote, JobsNote, LinksNote, MonthNote, MonthsNote, ReadingNote, RecentNote, StickyNote, ThenNote, TodoNote, YearNote } from "./notes";
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

// Where each note lies on the desk until it's moved (x, y, width): each half fits a 1200-wide window (the left half
// from 0, the right half from DESK_W − 1200 = 1360, where「右边」slides to).
const SPOT: Record<string, [number, number, number]> = {
  brief: [48, 96, 476],
  todo: [548, 26, 312],
  days: [880, 14, 296],
  note: [48, 404, 236],
  year: [300, 372, 300],
  reading: [620, 380, 280],
  assets: [918, 262, 262],
  month: [1384, 24, 290],
  months: [1700, 20, 420],
  jobs: [2140, 30, 330],
  drawing: [1384, 360, 300],
  recent: [1704, 304, 330],
  then: [2054, 340, 230],
  links: [2300, 330, 244],
};

export default function HomePage() {
  const now = useNow();
  const today = toYmd(now);
  const [hidden] = useHiddenModules();
  const { data, refetch } = useOverview(hidden);
  const agenda = useAgenda(today);
  const open = useOpen();
  const tabs = useTabs();
  const shown = (id: string) => !hidden.includes(id);
  // ticking a task here changes 最近 and 今年 too
  const seen = useRef(agenda.dataUpdatedAt);
  useEffect(() => {
    if (agenda.dataUpdatedAt === seen.current) return;
    seen.current = agenda.dataUpdatedAt;
    void refetch();
  }, [agenda.dataUpdatedAt, refetch]);

  let n = 0;
  const note = (id: string, label: string, el: React.ReactNode) => {
    if (!el) return null;
    const [x, y, w] = SPOT[id];
    return (
      <DeskNote key={id} id={id} x={x} y={y} w={w} i={n++} label={label}>
        {el}
      </DeskNote>
    );
  };
  const readTitle = hidden.includes("films") ? "在读" : hidden.includes("books") ? "在看" : "在读在看";

  return (
    // 打开的问候 (motion part greeting): the notes fall onto the desk one by one, the greeting is written out and a
    // marker swipes under it, then 柚子 winks; each time you come back to the desk
    <div className="pm-greet is-in flex h-full min-h-0 flex-col">
      <Desk>
        <h1 className="dk-greet">
          <span className="gw pm-greet-word">
            <span className="pm-mark" data-hand="marker" data-seed="710">
              {greeting(now.getHours())}
            </span>
          </span>
          <span className="gs pm-greet-sub">今天也慢慢来。</span>
        </h1>
        {shown("youzi") &&
          note(
            "brief",
            "今日简报",
            <>
              <YearCard />
              <BriefCard today={today} onTitle={() => open.module("youzi")} />
            </>,
          )}
        {shown("todo") && note("todo", "今天的待办", <TodoNote today={today} onOpen={(t) => open.record(taskHit(t))} go={() => open.module("todo")} />)}
        {data?.days && data.days.length > 0 && note("days", "快到的日子", <DaysNote days={data.days} hit={dayHit} open={open.record} go={() => open.module("days")} />)}
        {data?.reading && note("reading", readTitle, <ReadingNote items={data.reading} title={readTitle} open={open.record} go={() => open.module(shown("books") ? "books" : "films")} />)}
        {data?.assets && data.assets.length > 0 && note("assets", "资产小票", <AssetsNote points={data.assets} go={() => open.module("finance", "assets")} />)}
        {data?.note && note("note", "念念不忘一条", <StickyNote note={data.note} open={() => open.record({ ...BLANK, module: "notes", kind: "note", id: data.note!.id, title: data.note!.content.slice(0, 40) })} />)}
        {data && note("year", "今年的数字", <YearNote year={data.year} go={(id) => open.module(id)} />)}
        {data && note("month", "本月", <MonthNote today={today} entries={data.month} open={() => tabs.open("/calendar")} />)}
        {data && note("months", "今年按月", <MonthsNote year={data.year.year} months={data.months} month={now.getMonth()} />)}
        {data?.jobs && note("jobs", "求职近况", <JobsNote card={data.jobs} open={open.record} go={() => open.module("jobs")} />)}
        {data?.drawing && note("drawing", "绘画进度", <DrawingNote courses={data.drawing} open={open.record} go={() => open.module("drawing")} />)}
        {data && data.recent.length > 0 && note("recent", "最近记录", <RecentNote items={data.recent} when={(at) => when(at, today)} open={open.record} />)}
        {data && data.then.length > 0 && note("then", "那年今日", <ThenNote years={data.then} open={open.record} />)}
        {data?.bookmarks && data.bookmarks.length > 0 && note("links", "一网打尽", <LinksNote links={data.bookmarks} go={() => open.module("bookmarks")} />)}
      </Desk>
    </div>
  );
}

const dayHit = (d: Day): Hit => ({ ...BLANK, module: "days", kind: "day", id: d.id, title: d.name });
