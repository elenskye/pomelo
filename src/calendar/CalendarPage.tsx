// 日历 (docs/requirements.md「以后 › 日历视图」, design.md「日历」; visual-atelier's canvases/pomelo-pages Calendar):
// a page, not a module — the desk's 本月小月历 opens it (站主 2026-11-08). 一步一步、来日方长、出门走走、投石问路 on one
// month (calendar.rs decides what goes where), read-only: 6 weeks from Monday, each day lists its things (3 at most,
// else 2 and「还有 N 项」); ‹ › change the month,「今天」comes back, the chips choose which kinds show. Click one: a
// card with what it is, and「去某模块看这一条」opens the record there.
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { useHiddenModules } from "@/app/hidden";
import { ModuleIcon } from "@/app/ModuleIcon";
import { moduleById } from "@/app/modules";
import { CAL_TINT } from "@/home/notes";
import { toYmd } from "@/shared/dates";
import { HandIcon } from "@/shared/handParts";
import { PageHeader } from "@/shared/ui";
import { useCalendar, type CalendarEntry } from "./api";
import "./calendar.css";

const NAMES: Record<CalendarEntry["source"], string> = { todo: "待办", day: "日子", trip: "出行", job: "面试" };
const STATE: Record<CalendarEntry["state"], string> = { due: "截止", next: "重复的下一次", done: "做完了", day: "纪念日", birthday: "生日", document: "证件到期", depart: "出发", active: "在进行", got: "拿到" };
const WEEK = ["一", "二", "三", "四", "五", "六", "日"];

export default function CalendarPage() {
  const today = toYmd(new Date());
  const [ym, setYm] = useState(() => ({ y: Number(today.slice(0, 4)), m: Number(today.slice(5, 7)) - 1 }));
  const [off, setOff] = useState<CalendarEntry["source"][]>([]);
  const [pop, setPop] = useState<{ e: CalendarEntry; x: number; y: number } | null>(null);
  const [hidden] = useHiddenModules();
  const navigate = useNavigate();
  const box = useRef<HTMLDivElement>(null);

  const days = useMemo(() => {
    const first = new Date(ym.y, ym.m, 1);
    const lead = (first.getDay() + 6) % 7;
    return Array.from({ length: 42 }, (_, i) => new Date(ym.y, ym.m, 1 - lead + i));
  }, [ym]);
  const from = toYmd(days[0]);
  const to = toYmd(days[41]);
  const { data } = useCalendar(from, to, hidden);
  const byDay = useMemo(() => {
    const m = new Map<string, CalendarEntry[]>();
    for (const e of data ?? []) {
      if (off.includes(e.source)) continue;
      if (!m.has(e.day)) m.set(e.day, []);
      m.get(e.day)!.push(e);
    }
    return m;
  }, [data, off]);
  const inMonth = (data ?? []).filter((e) => e.day.slice(0, 7) === `${ym.y}-${String(ym.m + 1).padStart(2, "0")}`).length;
  const todays = (data ?? []).filter((e) => e.day === today).length;

  useEffect(() => {
    if (!pop) return;
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setPop(null);
    window.addEventListener("keydown", esc);
    return () => window.removeEventListener("keydown", esc);
  }, [pop]);

  const step = (d: number) => {
    setPop(null);
    setYm(({ y, m }) => {
      const k = m + d;
      return { y: y + Math.floor(k / 12), m: ((k % 12) + 12) % 12 };
    });
  };
  const show = (e: CalendarEntry, el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    const b = box.current!.getBoundingClientRect();
    let y = r.bottom - b.top + 6;
    if (y + 170 > b.height) y = r.top - b.top - 170;
    setPop({ e, x: Math.max(0, Math.min(r.left - b.left, b.width - 290)), y });
  };
  const sources = (Object.keys(NAMES) as CalendarEntry["source"][]).filter((s) => !hidden.includes({ todo: "todo", day: "days", trip: "packing", job: "jobs" }[s]));

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader title="日历" right={<span>{`${ym.m + 1} 月 · ${inMonth} 件事${todays ? ` · 今天 ${todays} 件` : ""}`}</span>} />
      <div ref={box} className="cal flex min-h-0 flex-1 flex-col px-8 pb-4" onClick={(e) => !(e.target as Element).closest(".ce,.calpop") && setPop(null)}>
        <div className="calbar">
          <button type="button" className="pm-btn pm-btn--icon calnav" data-hand="round" data-seed={1401} aria-label="上个月" onClick={() => step(-1)}>
            <HandIcon name="back" size={18} seed={1402} />
          </button>
          <span className="calm pm-num">
            {ym.y} 年 {ym.m + 1} 月
          </span>
          <button type="button" className="pm-btn pm-btn--icon calnav" data-hand="round" data-seed={1403} aria-label="下个月" onClick={() => step(1)}>
            <HandIcon name="fwd" size={18} seed={1404} />
          </button>
          <button type="button" className="pm-btn pm-btn--plain calnow" data-hand="button" data-fill="card" data-seed={1405} onClick={() => (setPop(null), setYm({ y: Number(today.slice(0, 4)), m: Number(today.slice(5, 7)) - 1 }))}>
            今天
          </button>
          <span className="calf" role="group" aria-label="看哪些">
            {sources.map((s) => (
              <button key={s} type="button" className={`pm-chip pm-cat-${CAL_TINT[s]} calsrc`} aria-pressed={!off.includes(s)} onClick={() => setOff(off.includes(s) ? off.filter((x) => x !== s) : [...off, s])}>
                {NAMES[s]}
              </button>
            ))}
          </span>
        </div>
        <div className="pm-card calcard" data-hand="card" data-seed={1406}>
          <div className="calhead">
            {WEEK.map((w) => (
              <span key={w}>{w}</span>
            ))}
          </div>
          <div className="calcells">
            {days.map((d, i) => {
              const key = toYmd(d);
              const es = byDay.get(key) ?? [];
              const list = es.length <= 3 ? es : es.slice(0, 2);
              return (
                <div key={key} className={`cc ${d.getMonth() !== ym.m ? "out" : ""} ${i % 7 >= 5 ? "we" : ""} ${key === today ? "now" : ""}`}>
                  <span className="cd">
                    {key === today && <span className="cnow" data-hand="frame" data-r="half" data-sw="1.8" data-amp="0.7" data-seed={1410} />}
                    <span className="relative">{d.getDate()}</span>
                  </span>
                  {list.map((e, k) => {
                    const late = e.source === "todo" && e.state === "due" && e.day < today;
                    return (
                      <button
                        key={`${e.module}${e.kind}${e.id}${k}`}
                        type="button"
                        title={e.note ? `${e.title} · ${e.note}` : e.title}
                        className={`ce pm-cat-${CAL_TINT[e.source]} ${e.state === "done" ? "done" : e.state === "next" ? "next" : late ? "late" : ""}`}
                        onClick={(ev) => show(e, ev.currentTarget)}
                      >
                        <i />
                        <span>{e.title}</span>
                      </button>
                    );
                  })}
                  {es.length > list.length && <div className="cmore">还有 {es.length - list.length} 项</div>}
                </div>
              );
            })}
          </div>
        </div>
        {pop && (
          <div className="calpop pm-card" data-hand="card" data-seed={1407} style={{ left: pop.x, top: pop.y }}>
            <div className="flex items-center gap-2">
              <ModuleIcon def={moduleById(pop.e.module)} size={24} />
              <span className="text-[13px] text-[var(--color-ink-soft)]">
                {moduleById(pop.e.module).label} · {STATE[pop.e.state]}
              </span>
            </div>
            <div className="pt">{pop.e.title}</div>
            <div className="text-[13px] text-[var(--color-ink-soft)]">
              {Number(pop.e.day.slice(5, 7))} 月 {Number(pop.e.day.slice(8))} 日{pop.e.note ? ` · ${pop.e.note}` : ""}
            </div>
            <button type="button" className="pm-link" onClick={() => void navigate(`/${pop.e.module}`, { state: { open: pop.e } })}>
              去「{moduleById(pop.e.module).label}」看这一条
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
