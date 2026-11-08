// The notes on the desk (docs/design.md「首页」; visual-atelier's canvases/pomelo-pages Desk / DeskRight). Each is the
// Design System's card, sticky note or receipt (ds.css pm-*, painted by shared/hand.ts). A note's title opens its
// module, a row opens its record (the same hand-off as 全局搜索). Data: home_overview (home/api.ts) and today's agenda.
import { useState, type ReactNode } from "react";
import { ModuleIcon } from "@/app/ModuleIcon";
import { moduleById } from "@/app/modules";
import { ownTint } from "@/app/looks";
import { useModuleLooks } from "@/app/moduleLooks";
import type { CalendarEntry } from "@/calendar/api";
import { useSiteIcons, type Bookmark } from "@/modules/bookmarks/api";
import { SiteIcon } from "@/modules/bookmarks/SiteIcon";
import { countText, nthText, type Day } from "@/modules/days/api";
import { money, type Currency } from "@/modules/finance/money";
import { fetchAgenda, useAgenda, useTodoMutations, type Task } from "@/modules/todo/api";
import { ActionCircle } from "@/modules/todo/TaskList";
import { cheerOver } from "@/shared/cheer";
import { errorText, useToast } from "@/shared/feedback";
import { flash } from "@/shared/hand";
import { HandIcon, Tape, Tip } from "@/shared/handParts";
import { InlineMd } from "@/shared/InlineMd";
import { api as ipc } from "@/shared/ipc";
import type { Hit } from "@/shared/open";
import type { CourseLine, DeskNote as DeskNoteData, JobsCard, Months, Point, Recent, Then, Year } from "./api";

const f = (n: number) => +n.toFixed(2);
type Hand = { P: (d: string) => unknown; handFill: (c: unknown, o: object) => string; handStroke: (c: unknown, o: object) => string };
const hand = () => (window as unknown as { Pomelo?: { hand?: Hand } }).Pomelo?.hand ?? null;
const stroke = (d: string, o: object) => {
  const h = hand();
  return h ? h.handStroke(h.P(d), { sp: 1.5, keep: 3, ...o }) : d;
};
const fill = (d: string, o: object) => {
  const h = hand();
  return h ? h.handFill(h.P(d), { sp: 3, keep: 3, ...o }) : d;
};

/** A note's title: the module's sticker and name; opens the module. */
function Head(props: { module?: string; title: string; meta?: ReactNode; onTitle?: () => void; size?: number }) {
  const title = (
    <>
      {props.module && <ModuleIcon def={moduleById(props.module)} size={props.size ?? 26} />}
      <span className="pm-card-title">{props.title}</span>
    </>
  );
  return (
    <div className="dk-head">
      {props.onTitle ? (
        <button type="button" className="dk-title" onClick={props.onTitle}>
          {title}
        </button>
      ) : (
        <span className="dk-title">{title}</span>
      )}
      {props.meta && <span className="pm-card-meta">{props.meta}</span>}
    </div>
  );
}

/** A Design System card on the desk: tilted a little, a strip or two of tape. */
function Card(props: { seed: number; tilt: number; tapes?: number; tint?: string; lined?: boolean; className?: string; children: ReactNode }) {
  return (
    <article
      className={`pm-card dk-card ${props.lined ? "pm-card--lined dk-lined" : ""} ${props.className ?? ""}`}
      data-hand="card"
      data-seed={props.seed}
      data-tilt={props.tilt}
      data-tapes={props.tapes ?? 1}
      style={props.tint ? ({ "--pm-card-bg": `var(--color-cat-${props.tint}-tint)` } as React.CSSProperties) : undefined}
    >
      {props.children}
    </article>
  );
}

// ---------- 今天的待办 ----------
export function TodoNote(props: { today: string; onOpen: (t: Task) => void; go: () => void }) {
  const { data: a } = useAgenda(props.today);
  const m = useTodoMutations();
  if (!a) return null;
  const open = [...a.overdue, ...a.due];
  const rows = [...open, ...a.doneToday].slice(0, 5);
  const more = open.length + a.doneToday.length - rows.length;
  const toggle = (t: Task, el: HTMLElement | null) => {
    const done = !(t.done || (t.taskType === "checkin" && t.loggedToday));
    m.setDone.mutate(
      { id: t.id, done },
      {
        onSuccess: () => {
          if (!done) return;
          if (t.taskType !== "checkin") flash(el, "is-pop", 700);
          void fetchAgenda(props.today)
            .then((x) => {
              if (!x.overdue.length && !x.due.length && x.doneToday.length) cheerOver(el?.closest("[data-tasks]") as HTMLElement, "今天的事都做完啦！");
            })
            .catch(() => {});
        },
      },
    );
  };
  return (
    <Card seed={743} tilt={1.4} tapes={2} lined className="dk-todo">
      <Head module="todo" title="今天的待办" meta={`${a.doneToday.length} / ${open.length + a.doneToday.length}`} onTitle={props.go} />
      <div data-tasks>
        {rows.length === 0 && <p className="dk-empty">今天没有安排，喝杯茶吧</p>}
        {rows.map((t) => {
          const struck = t.done && t.taskType !== "checkin";
          const late = !t.done && t.dueDate !== null && t.dueDate < props.today;
          return (
            <div key={t.id} className="dk-row">
              <ActionCircle task={t} onToggle={() => toggle(t, document.getElementById(`dk-t${t.id}`))} />
              <button type="button" className="t" onClick={() => props.onOpen(t)}>
                <span id={`dk-t${t.id}`} className={`task-title ${struck ? "done" : ""}`} style={struck ? { color: "var(--color-ink-soft)" } : undefined} data-hand="strike" data-seed={1405 + (t.id % 7)}>
                  <InlineMd text={t.title} />
                </span>
              </button>
              {t.dueDate && t.dueDate !== props.today && <span className={`due ${late ? "late" : ""}`}>{t.dueDate.slice(5).replace("-", " / ")}</span>}
            </div>
          );
        })}
        {more > 0 && (
          <button type="button" className="dk-more" onClick={props.go}>
            还有 {more} 件
          </button>
        )}
      </div>
    </Card>
  );
}

// ---------- 快到的日子 ----------
export function DaysNote(props: { days: Day[]; open: (h: Hit) => void; go: () => void; hit: (d: Day) => Hit }) {
  if (props.days.length === 0) return null;
  return (
    <Card seed={744} tilt={-1.8} tint="pink">
      <Head module="days" title="快到的日子" onTitle={props.go} />
      {props.days.slice(0, 4).map((d, i) => {
        const [before, n, after] = countText(d);
        const [, mm, dd] = d.next.split("-");
        const nth = nthText(d);
        return (
          <button key={d.id} type="button" className="dk-day" onClick={() => props.open(props.hit(d))}>
            <span className="n">
              <span>{d.name}</span>
              <span>
                {+mm} 月 {+dd} 日{nth ? ` · ${nth}` : ""}
              </span>
            </span>
            <span className="pm-stat" style={d.days < 0 ? { color: "var(--color-danger)" } : undefined}>
              {before}{" "}
              {n !== null &&
                (i === 0 ? (
                  <span className="pm-mark" data-hand="marker" data-seed={765}>
                    <span className="pm-stat-num">{n}</span>
                  </span>
                ) : (
                  <span className="pm-stat-num">{n}</span>
                ))}{" "}
              {after}
            </span>
          </button>
        );
      })}
    </Card>
  );
}

// ---------- 在读在看 ----------
export function ReadingNote(props: { items: Hit[]; title: string; open: (h: Hit) => void; go: () => void }) {
  return (
    <Card seed={745} tilt={-1} tapes={2}>
      <Head module={props.items[0]?.module ?? "books"} title={props.title} onTitle={props.go} />
      {props.items.length === 0 && <p className="dk-empty">这会儿没有{props.title}的</p>}
      {props.items.slice(0, 5).map((h) => (
        <button key={`${h.kind}${h.id}`} type="button" className="dk-row" onClick={() => props.open(h)}>
          <ModuleIcon def={moduleById(h.module)} size={20} />
          <span className="t">《{h.title}》</span>
          {h.detail && <span className="d">{h.detail}</span>}
        </button>
      ))}
    </Card>
  );
}

// ---------- 资产小票：翻过来是曲线 ----------
function curve(points: Point[], w: number, h: number) {
  const vals = points.map((p) => p.total);
  const lo = Math.min(...vals);
  const hi = Math.max(...vals);
  const span = hi - lo || 1;
  const xy = vals.map((v, i) => [6 + (i * (w - 12)) / Math.max(1, vals.length - 1), 6 + (h - 12) * (1 - (v - lo) / span)]);
  const line = "M" + xy.map(([x, y]) => `${f(x)} ${f(y)}`).join(" L");
  const area = `${line} L${w - 6} ${h} L6 ${h} Z`;
  const [lx, ly] = xy[xy.length - 1];
  return (
    <svg width={w} height={h} aria-hidden style={{ display: "block", overflow: "visible" }}>
      <path d={fill(area, { amp: 0.6, seed: 90 })} fill="var(--color-cat-mint-tint)" />
      <path d={stroke(line, { amp: 0.8, seed: 91, wave: 14 })} fill="none" stroke="var(--color-pen)" strokeWidth={2.2} strokeLinecap="round" strokeLinejoin="round" />
      <circle cx={lx} cy={ly} r={4.5} fill="var(--color-yuzu)" stroke="var(--color-pen)" strokeWidth={1.8} />
    </svg>
  );
}

const ymdText = (d: string) => `${+d.slice(5, 7)} 月 ${+d.slice(8)} 日`;

export function AssetsNote(props: { points: Point[]; go: () => void }) {
  const [back, setBack] = useState(false);
  const p = props.points;
  const last = p[p.length - 1];
  const prev = p[p.length - 2];
  if (!last) return null;
  const delta = prev ? last.total - prev.total : null;
  const rows = p.slice(-4, -1).reverse();
  return (
    <div className="dk-receipt" style={{ width: 262, height: 320 }}>
      <div className={`pm-flip ${back ? "is-flipped" : ""}`}>
        <div className="pm-flip-face">
          <button type="button" className="pm-receipt" style={{ width: 262, height: 320, textAlign: "left" }} onClick={props.go}>
            <div className="pm-receipt-head">
              — 柚儿园 · 资产小票 —<br />
              {ymdText(last.day)}
            </div>
            <span className="pm-receipt-cut" />
            {rows.map((r) => (
              <div key={r.day} className="pm-receipt-row">
                <span>{ymdText(r.day)}</span>
                <span className="pm-num">{money(r.total, "CNY")}</span>
              </div>
            ))}
            <span className="pm-receipt-cut" />
            <div className="pm-receipt-row pm-receipt-total">
              <span>合计</span>
              <span className="pm-num" style={{ fontSize: 22 }}>
                {money(last.total, "CNY")}
              </span>
            </div>
            {delta !== null && (
              <div className="pm-receipt-row">
                <span />
                <span className={delta >= 0 ? "pm-up" : ""} style={delta < 0 ? { color: "var(--color-danger)" } : undefined}>
                  比上次 {delta >= 0 ? "+" : "−"}
                  {money(Math.abs(delta), "CNY")}
                </span>
              </div>
            )}
          </button>
        </div>
        <div className="pm-flip-face pm-flip-back">
          <div className="pm-receipt" style={{ width: 262, height: 320 }}>
            <div style={{ fontSize: 16, fontWeight: 700, marginBottom: 8 }}>这几次的余额</div>
            {p.length > 1 ? curve(p.slice(-12), 214, 150) : <p className="dk-empty">记过两次以后有曲线</p>}
            <div className="flex justify-between" style={{ width: 214, marginTop: 4, fontSize: 12, color: "var(--color-ink-soft)" }}>
              <span>{ymdText(p[Math.max(0, p.length - 12)].day)}</span>
              <span>{ymdText(last.day)}</span>
            </div>
          </div>
        </div>
      </div>
      <span className="rtape">
        <Tape kind="yuzu" rot={4} />
      </span>
      <button type="button" className="rflip pm-press pm-tip-host" aria-label="翻面" onClick={() => setBack(!back)}>
        <HandIcon name="flip" size={20} seed={790} />
        <Tip>翻面</Tip>
      </button>
    </div>
  );
}

// ---------- 念念不忘一条 ----------
export function StickyNote(props: { note: DeskNoteData; open: () => void }) {
  return (
    <div className="pm-note dk-sticky" data-hand="note" data-seed={746} data-tilt={-2.2} data-tapes={1}>
      <div className="dk-head" style={{ marginBottom: 6 }}>
        <ModuleIcon def={moduleById("notes")} size={24} />
        <span style={{ fontSize: 15, fontWeight: 700 }}>念念不忘</span>
      </div>
      <button type="button" className="txt" onClick={props.open}>
        {props.note.content}
      </button>
      <div style={{ fontSize: 12, marginTop: 8, opacity: 0.7 }}>{ymdText(props.note.day)}</div>
    </div>
  );
}

// ---------- 今年的数字：一圈圆贴纸 ----------
const YEAR_SPOTS: [number, number, number][] = [
  [0, 28, -7],
  [96, 2, 5],
  [192, 26, -4],
  [14, 126, 4],
  [110, 112, -6],
  [206, 130, 6],
];
export function YearNote(props: { year: Year; go: (id: string) => void }) {
  const { eff } = useModuleLooks();
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
    <div className="dk-year" style={{ width: 300 }}>
      <div className="yt">{Number(y.year)} 年 · 到今天</div>
      {cells.map(([id, label, n, unit], i) => {
        const [x, top, r] = YEAR_SPOTS[i];
        const tint = eff.get(id)?.tint ?? ownTint(moduleById(id)) ?? "sky";
        return (
          <button
            key={id}
            type="button"
            className="dk-ystat"
            data-hand="round"
            data-fill={`cat-${tint}-sticker`}
            data-sw="1.6"
            data-seed={780 + i}
            data-tilt={r}
            style={{ left: x, top: top + 24 }}
            onClick={() => props.go(id)}
            aria-label={`${label} ${n} ${unit}`}
          >
            <ModuleIcon def={moduleById(id)} size={22} />
            <span className="v">
              {n}
              <small>{unit}</small>
            </span>
            <span className="l">{label}</span>
          </button>
        );
      })}
    </div>
  );
}

// ---------- 本月小月历：哪天有事打点，今天画圈；点了打开日历 ----------
export const CAL_TINT: Record<CalendarEntry["source"], string> = { todo: "mint", day: "pink", trip: "orange", job: "grape" };
export function MonthNote(props: { today: string; entries: CalendarEntry[]; open: () => void }) {
  const [yy, mm, dd] = props.today.split("-").map(Number);
  const first = new Date(yy, mm - 1, 1);
  const lead = (first.getDay() + 6) % 7;
  const days = new Date(yy, mm, 0).getDate();
  const by = new Map<number, Set<CalendarEntry["source"]>>();
  for (const e of props.entries) {
    if (!e.day.startsWith(props.today.slice(0, 7))) continue;
    const d = Number(e.day.slice(8));
    if (!by.has(d)) by.set(d, new Set());
    by.get(d)!.add(e.source);
  }
  const n = props.entries.filter((e) => e.day.startsWith(props.today.slice(0, 7))).length;
  return (
    <Card seed={1310} tilt={1.2} className="dk-month">
      <button type="button" style={{ display: "block", width: "100%", textAlign: "left" }} onClick={props.open} aria-label="打开日历">
        <div className="dk-head" style={{ marginBottom: 0 }}>
          <span className="pm-card-title">{mm} 月</span>
          <span className="pm-card-meta">{n > 0 ? `${n} 件事` : "还空着"}</span>
        </div>
        <div className="dk-mgrid">
          {["一", "二", "三", "四", "五", "六", "日"].map((w) => (
            <span key={w} className="mh">
              {w}
            </span>
          ))}
          {Array.from({ length: lead }, (_, i) => (
            <span key={`b${i}`} />
          ))}
          {Array.from({ length: days }, (_, i) => {
            const d = i + 1;
            const we = (lead + i) % 7 >= 5;
            return (
              <span key={d} className={`mc ${we ? "we" : ""} ${d === dd ? "now" : ""}`}>
                {d === dd && <span className="mnow" data-hand="frame" data-r="half" data-sw="1.7" data-amp="0.7" data-seed={1312} />}
                <b>{d}</b>
                <span className="dots">
                  {[...(by.get(d) ?? [])].map((s) => (
                    <i key={s} className={`pm-cat-${CAL_TINT[s]}`} />
                  ))}
                </span>
              </span>
            );
          })}
        </div>
      </button>
    </Card>
  );
}

// ---------- 今年按月：一次看一种，柱子手画、当月的写上数 ----------
const SERIES: { key: keyof Months; name: string; unit: string; tint: string; module: string }[] = [
  { key: "todos", name: "完成", unit: "件", tint: "mint", module: "todo" },
  { key: "notes", name: "笔记", unit: "条", tint: "sky", module: "notes" },
  { key: "lessons", name: "画完", unit: "节", tint: "pink", module: "drawing" },
  { key: "jobs", name: "投出", unit: "份", tint: "grape", module: "jobs" },
];
export function MonthsNote(props: { year: string; months: Months; month: number }) {
  const shown = SERIES.filter((s) => props.months[s.key] !== null);
  const [pick, setPick] = useState<keyof Months | null>(null);
  if (shown.length === 0) return null;
  const s = shown.find((x) => x.key === pick) ?? shown[0];
  const v = props.months[s.key]!;
  const W = 372;
  const H = 128;
  const bw = 20;
  const gap = (W - 12 * bw) / 12;
  const max = Math.max(1, ...v);
  const total = v.reduce((a, b) => a + b, 0);
  return (
    <Card seed={1320} tilt={-0.8} tapes={2} className="dk-months">
      <div className="dk-head" style={{ marginBottom: 0 }}>
        <span className="pm-card-title">{Number(props.year)} 年 · 每个月</span>
        <span className="pm-card-meta">
          共 <b className="pm-num" style={{ color: "var(--color-ink)" }}>{total}</b> {s.unit}
        </span>
      </div>
      <div className="sers" role="group" aria-label="看哪一种">
        {shown.map((x) => (
          <button key={x.key} type="button" className={`pm-chip pm-cat-${x.tint}`} aria-pressed={x.key === s.key} onClick={() => setPick(x.key)}>
            {x.name}
          </button>
        ))}
      </div>
      <svg width={W} height={H} viewBox={`0 0 ${W} ${H}`} aria-label={`${s.name}，每个月`} style={{ overflow: "visible", display: "block" }}>
        {Array.from({ length: 12 }, (_, i) => {
          const val = v[i] ?? 0;
          const x = gap / 2 + i * (bw + gap);
          if (i > props.month || !val) return <path key={i} d={stroke(`M${f(x + 2)} ${H - 1} L${f(x + bw - 2)} ${H - 1}`, { amp: 0.4, seed: 1330 + i })} stroke="var(--color-line)" strokeWidth={2} strokeLinecap="round" fill="none" />;
          const bh = Math.max(6, (val / max) * (H - 22));
          const y = H - bh;
          const shape = `M${f(x)} ${f(y + 5)} Q${f(x)} ${f(y)} ${f(x + 5)} ${f(y)} L${f(x + bw - 5)} ${f(y)} Q${f(x + bw)} ${f(y)} ${f(x + bw)} ${f(y + 5)} L${f(x + bw)} ${H} L${f(x)} ${H} Z`;
          const now = i === props.month;
          return (
            <g key={`${s.key}${i}`}>
              <title>
                {i + 1} 月 {val} {s.unit}
              </title>
              <path d={fill(shape, { amp: 0.5, seed: 1330 + i })} fill={`var(--color-cat-${s.tint}${now ? "" : "-sticker"})`} />
              <path d={stroke(shape, { amp: 0.5, seed: 1331 + i, wave: 12 })} fill="none" stroke="var(--color-pen)" strokeWidth={now ? 1.8 : 1.3} strokeLinejoin="round" />
              {now && (
                <text x={f(x + bw / 2)} y={f(y - 6)} textAnchor="middle">
                  {val}
                </text>
              )}
            </g>
          );
        })}
      </svg>
      <div className="lab">
        {Array.from({ length: 12 }, (_, i) => (
          <span key={i} className={i === props.month ? "on" : ""}>
            {i + 1}
          </span>
        ))}
      </div>
    </Card>
  );
}

// ---------- 求职近况 ----------
export function JobsNote(props: { card: JobsCard; open: (h: Hit) => void; go: () => void }) {
  const c = props.card;
  const lines = [...c.follow, ...c.active];
  const nums: [number, string, string][] = [
    [c.thisWeek, "份", "本周投了"],
    [c.ongoing, "个", "进行中"],
    [c.got, "个", "拿到"],
  ];
  return (
    <Card seed={1340} tilt={1.6} tint="grape">
      <Head module="jobs" title="求职近况" meta={c.total > 0 ? `共 ${c.total} 条` : undefined} onTitle={props.go} />
      {c.total === 0 ? (
        <p className="dk-empty">还没有投递</p>
      ) : (
        <>
          <div className="dk-jnums">
            {nums.map(([n, u, l]) => (
              <div key={l}>
                <span className="pm-stat">
                  <span className="pm-stat-num">{n}</span>
                  {u}
                </span>
                <span className="soft">{l}</span>
              </div>
            ))}
          </div>
          {lines.slice(0, 4).map((j, i) => (
            <button key={j.id} type="button" className="dk-row" onClick={() => props.open(j)}>
              <span className="t">{j.title}</span>
              {j.stage === "applied" ? (
                <span className="d">
                  <span className="pm-mark" data-hand="marker" data-seed={1341 + i}>
                    该跟进 · 等了 {j.waitingDays} 天
                  </span>
                </span>
              ) : (
                <span className={`pm-chip pm-cat-${j.stage === "got" ? "mint" : j.stage === "closed" ? "pink" : j.stage === "wish" ? "sky" : "yuzu"}`}>{j.status}</span>
              )}
            </button>
          ))}
        </>
      )}
    </Card>
  );
}

// ---------- 绘画进度 ----------
export function DrawingNote(props: { courses: CourseLine[]; open: (h: Hit) => void; go: () => void }) {
  return (
    <Card seed={1350} tilt={-1.4}>
      <Head module="drawing" title="绘画进度" onTitle={props.go} />
      {props.courses.length === 0 && <p className="dk-empty">没有在学的课</p>}
      {props.courses.slice(0, 3).map((c, i) => (
        <button key={c.id} type="button" style={{ display: "block", width: "100%", textAlign: "left", marginTop: 8 }} onClick={() => (c.next ? props.open(c.next) : props.go())}>
          <span className="flex items-baseline" style={{ fontSize: 15, lineHeight: "22px" }}>
            <span className="truncate">{c.name}</span>
            <span className="pm-num" style={{ marginLeft: "auto", fontSize: 13, color: "var(--color-ink-soft)" }}>
              {c.done} / {c.total}
            </span>
          </span>
          <span className="dk-bar block" data-hand="frame" data-r="6" data-sw="1.4" data-amp="0.4" data-fill="card" data-seed={1352 + i}>
            <span className="in" style={{ width: `${c.total ? Math.round((c.done / c.total) * 100) : 0}%`, background: "var(--color-cat-pink)" }} />
          </span>
          {c.next && <span className="block truncate" style={{ fontSize: 12, marginTop: 3, color: "var(--color-ink-soft)" }}>下一节 · {c.next.title}</span>}
        </button>
      ))}
    </Card>
  );
}

// ---------- 最近记录 ----------
function recentDetail(r: Recent) {
  if (r.kind === "entry" && r.amount !== null && r.currency) return money(r.amount, r.currency as Currency, r.flow === "in");
  if (r.kind === "task") return "完成";
  return null;
}
export function RecentNote(props: { items: Recent[]; when: (at: string) => string; open: (h: Hit) => void }) {
  if (props.items.length === 0) return null;
  return (
    <Card seed={1360} tilt={0.8} lined>
      <div className="pm-card-title">最近记录</div>
      {props.items.slice(0, 6).map((r) => {
        const d = recentDetail(r);
        return (
          <button key={`${r.kind}${r.id}`} type="button" className="dk-row" onClick={() => props.open(r)}>
            <ModuleIcon def={moduleById(r.module)} size={20} />
            <span className="t">
              {r.title}
              {d && <span style={{ color: "var(--color-ink-soft)" }}> · {d}</span>}
            </span>
            <span className="d">{props.when(r.at)}</span>
          </button>
        );
      })}
    </Card>
  );
}

// ---------- 那年今日：一张小卡片，「换一条」 ----------
export function ThenNote(props: { years: Then[]; open: (h: Hit) => void }) {
  const [at, setAt] = useState(0);
  const all = props.years.flatMap((y) => y.items.map((h) => ({ year: y.year, h })));
  if (all.length === 0) return null;
  const { year, h } = all[at % all.length];
  return (
    <Card seed={1370} tilt={-2.4} tint="orange" className="dk-then">
      <div className="dk-head" style={{ marginBottom: 0 }}>
        <span className="pm-card-title">那年今日</span>
        <span className="pm-chip pm-cat-orange" style={{ marginLeft: "auto" }}>
          {year}
        </span>
      </div>
      <button type="button" className="txt" onClick={() => props.open(h)}>
        {h.title}
      </button>
      <div className="dk-head" style={{ margin: "8px 0 0" }}>
        <ModuleIcon def={moduleById(h.module)} size={18} />
        <span style={{ fontSize: 12, color: "var(--color-ink-soft)" }}>{moduleById(h.module).label}</span>
        {all.length > 1 && (
          <button type="button" className="pm-link" style={{ marginLeft: "auto", fontSize: 13 }} onClick={() => setAt(at + 1)}>
            换一条
          </button>
        )}
      </div>
    </Card>
  );
}

// ---------- 一网打尽：放首页的网址 ----------
export function LinksNote(props: { links: Bookmark[]; go: () => void }) {
  const icons = useSiteIcons();
  const toast = useToast();
  if (props.links.length === 0) return null;
  const host = (u: string) => {
    try {
      return new URL(u).hostname.replace(/^www\./, "");
    } catch {
      return "";
    }
  };
  return (
    <Card seed={1380} tilt={1.8} className="dk-links">
      <Head module="bookmarks" title="一网打尽" size={24} onTitle={props.go} />
      {props.links.slice(0, 6).map((b) => (
        <button key={b.id} type="button" className="dk-row" title={b.url} onClick={() => void ipc.openLink(b.url).catch((e) => toast(errorText(e), "error"))}>
          <SiteIcon url={b.url} icons={icons} />
          <span className="t">{b.title}</span>
          <span className="d">{host(b.url)}</span>
        </button>
      ))}
    </Card>
  );
}
