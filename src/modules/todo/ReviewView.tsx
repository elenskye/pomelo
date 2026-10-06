// 回看: what got done in a week / month / year / all time. Counts per day or month (columns), per
// group (ranked bars), a few numbers, then the list itself. One "件" = one DoneItem (a checked task,
// or one occurrence / check-in / amount of a typed task), as Rust reports them.
import { useState } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { BarList, ColumnChart } from "@/shared/charts";
import { addDays, formatDay, mondayOf, parseYmd, toYmd, weekday } from "@/shared/dates";
import { Button, IconButton, Segmented } from "@/shared/ui";
import { useDoneBetween, type DoneItem, type Task } from "./api";
import { Section, ViewHeader } from "./layout";
import { PlacedList } from "./TaskList";

type Period = "week" | "month" | "year" | "all";
const PERIODS: { value: Period; label: string }[] = [
  { value: "week", label: "周" },
  { value: "month", label: "月" },
  { value: "year", label: "年" },
  { value: "all", label: "全部" },
];
const ALL_FROM = "2000-01-01";
const WEEK = ["一", "二", "三", "四", "五", "六", "日"];

const monthLabel = (ym: string) => `${Number(ym.slice(0, 4))} 年 ${Number(ym.slice(5, 7))} 月`;
const nextMonth = (ym: string) => toYmd(new Date(Number(ym.slice(0, 4)), Number(ym.slice(5, 7)), 1)).slice(0, 7);

function rangeOf(period: Period, offset: number, today: string) {
  const [y, m] = [Number(today.slice(0, 4)), Number(today.slice(5, 7))];
  switch (period) {
    case "week": {
      const from = addDays(mondayOf(today), offset * 7);
      const label = offset === 0 ? "本周" : offset === -1 ? "上周" : `${formatDay(from)} 那一周`;
      return { from, to: addDays(from, 6), label: `${label} · ${formatDay(from)} – ${formatDay(addDays(from, 6))}` };
    }
    case "month": {
      const first = new Date(y, m - 1 + offset, 1);
      const from = toYmd(first);
      return { from, to: toYmd(new Date(first.getFullYear(), first.getMonth() + 1, 0)), label: offset === 0 ? "本月" : monthLabel(from) };
    }
    case "year":
      return { from: `${y + offset}-01-01`, to: `${y + offset}-12-31`, label: offset === 0 ? "今年" : `${y + offset} 年` };
    case "all":
      return { from: ALL_FROM, to: today, label: "全部" };
  }
}

/** Buckets for the columns: days of the week / month, months of the year, months (or years) overall. */
function bucketsOf(period: Period, from: string, to: string, items: DoneItem[], today: string) {
  const days = (n: number) => Array.from({ length: n }, (_, i) => addDays(from, i));
  if (period === "week") return { keys: days(7), labels: WEEK, unit: "day" as const };
  if (period === "month") {
    const ks = days(parseYmd(to).getDate());
    return { keys: ks, labels: ks.map((d) => String(Number(d.slice(8)))), unit: "day" as const };
  }
  if (period === "year") {
    const ks = Array.from({ length: 12 }, (_, i) => `${from.slice(0, 4)}-${String(i + 1).padStart(2, "0")}`);
    return { keys: ks, labels: ks.map((k) => `${Number(k.slice(5))}月`), unit: "month" as const };
  }
  const first = items.length ? items.map((t) => t.day).reduce((a, b) => (a < b ? a : b)).slice(0, 7) : today.slice(0, 7);
  const months: string[] = [];
  for (let k = first; k <= today.slice(0, 7); k = nextMonth(k)) months.push(k);
  if (months.length <= 36) return { keys: months, labels: months.map((k) => (k.endsWith("-01") || k === months[0] ? `${k.slice(2, 4)}/${Number(k.slice(5))}` : `${Number(k.slice(5))}月`)), unit: "month" as const };
  const years = [...new Set(months.map((k) => k.slice(0, 4)))];
  return { keys: years, labels: years, unit: "year" as const };
}

export function ReviewView(props: { today: string; selectedId: number | null; onOpen: (t: Task) => void }) {
  const { today } = props;
  const [period, setPeriod] = useState<Period>("week");
  const [offset, setOffset] = useState(0);
  const range = rangeOf(period, offset, today);
  const { data: items = [], isPending } = useDoneBetween(range.from, range.to);
  const b = bucketsOf(period, range.from, range.to, items, today);
  const keyOf = (day: string) => (b.unit === "day" ? day : b.unit === "month" ? day.slice(0, 7) : day.slice(0, 4));
  const counts = b.keys.map((k) => items.filter((t) => keyOf(t.day) === k).length);

  // numbers: total, per-day / per-month average over the elapsed part, the busiest bucket
  const elapsedDays = Math.max(1, Math.round((parseYmd(range.to < today ? range.to : today).getTime() - parseYmd(range.from === ALL_FROM && items.length ? items.reduce((a, t) => (a < t.day ? a : t.day), today) : range.from).getTime()) / 86400000) + 1);
  const busiest = counts.reduce((best, n, i) => (n > counts[best] ? i : best), 0);
  const avg = b.unit === "day" ? `日均 ${(items.length / elapsedDays).toFixed(1)} 件` : `月均 ${(items.length / Math.max(1, counts.filter((_, i) => b.keys[i] <= keyOf(today)).length)).toFixed(1)} 件`;
  const busiestLabel =
    b.unit === "day" ? `${formatDay(b.keys[busiest])} ${weekday(b.keys[busiest])}` : b.unit === "month" ? monthLabel(b.keys[busiest]) : `${b.keys[busiest]} 年`;

  const byGroup = new Map<string, number>();
  for (const t of items) byGroup.set(t.groupName, (byGroup.get(t.groupName) ?? 0) + 1);
  const groups = [...byGroup.entries()].sort((x, y) => y[1] - x[1]);

  // list: by day for 周 / 月, by month for 年 / 全部 — newest first
  const listKey = (t: DoneItem) => (period === "week" || period === "month" ? t.day : t.day.slice(0, 7));
  const sections = new Map<string, DoneItem[]>();
  for (const t of [...items].reverse()) sections.set(listKey(t), [...(sections.get(listKey(t)) ?? []), t]);
  const sectionTitle = (k: string) => (k.length === 10 ? `${formatDay(k)} ${weekday(k)}` : monthLabel(k));
  const fmt = (n: number) => `${n} 件`;

  return (
    <>
      <ViewHeader
        title="回看"
        sub={`${range.label} · 完成了 ${items.length} 件`}
        actions={
          <>
            <Segmented
              ariaLabel="回看范围"
              value={period}
              onChange={(v) => {
                setPeriod(v);
                setOffset(0);
              }}
              options={PERIODS}
            />
            {period !== "all" && (
              <div className="flex items-center">
                <IconButton label="往前" onClick={() => setOffset((o) => o - 1)}>
                  <ChevronLeft size={16} />
                </IconButton>
                <Button variant="ghost" disabled={offset === 0} onClick={() => setOffset(0)}>
                  {{ week: "本周", month: "本月", year: "今年" }[period]}
                </Button>
                <IconButton label="往后" disabled={offset >= 0} onClick={() => setOffset((o) => o + 1)}>
                  <ChevronRight size={16} />
                </IconButton>
              </div>
            )}
          </>
        }
      />
      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto pb-4">
        {items.length === 0 && !isPending ? (
          <p className="py-10 text-center text-muted-foreground">这段时间还没有完成的事</p>
        ) : (
          <>
            <div className="grid grid-cols-[minmax(0,2fr)_minmax(0,1fr)] gap-4">
              <div className="rounded-card border bg-card px-4 pt-3 pb-2">
                <div className="mb-8 flex flex-wrap gap-x-4 text-sm text-muted-foreground">
                  <span>
                    完成 <b className="text-base text-foreground">{items.length}</b> 件
                  </span>
                  <span>{avg}</span>
                  {counts[busiest] > 0 && (
                    <span>
                      最多：{busiestLabel} {counts[busiest]} 件
                    </span>
                  )}
                </div>
                <ColumnChart
                  labels={b.labels}
                  series={[{ name: "完成", color: "var(--chart-1)" }]}
                  values={[counts]}
                  format={fmt}
                  axisFormat={String}
                  integer
                  labelEvery={b.keys.length > 16 ? 5 : 1}
                  title={(i) => (b.unit === "day" ? `${formatDay(b.keys[i])} ${weekday(b.keys[i])}` : b.unit === "month" ? monthLabel(b.keys[i]) : `${b.keys[i]} 年`)}
                />
              </div>
              <div className="rounded-card border bg-card px-4 py-3">
                <p className="mb-3 text-sm text-muted-foreground">按分组</p>
                <BarList rows={groups.map(([name, n]) => ({ key: name, label: name, value: n }))} format={fmt} />
              </div>
            </div>
            {[...sections.entries()].map(([k, ts]) => (
              <Section key={k} title={`${sectionTitle(k)} · ${ts.length} 件`}>
                <PlacedList items={ts} today={today} selectedId={props.selectedId} onOpen={props.onOpen} />
              </Section>
            ))}
          </>
        )}
      </div>
    </>
  );
}
