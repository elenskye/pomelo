// 月历: 6-week grid of dated work (plans, deadlines, time ranges, upcoming repeats, what got done).
// Click a day to list its items below the grid.
import { useState } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { addDays, formatDay, mondayOf, toYmd, weekday } from "@/shared/dates";
import { Button, IconButton } from "@/shared/ui";
import { useMonth, type CalItem, type Task } from "./api";
import { PlacedList } from "./TaskList";

const WEEK = ["一", "二", "三", "四", "五", "六", "日"];
const WHAT: Record<CalItem["what"], string> = { plan: "计划", due: "截止", range: "时间段", next: "之后的一次", done: "完成" };

function Chip({ it, today }: { it: CalItem; today: string }) {
  const late = (it.what === "due" || it.what === "plan") && it.day < today;
  const title = it.title.replace(/\*\*/g, "");
  return (
    <div
      title={`${WHAT[it.what]}：${title}${it.detail ? `（${it.detail}）` : ""}`}
      className={`truncate rounded px-1 text-[11px] leading-4 ${
        it.what === "done"
          ? "text-muted-foreground line-through decoration-1"
          : it.what === "next"
            ? "text-muted-foreground"
            : it.what === "range"
              ? "bg-muted"
              : late
                ? "text-destructive"
                : ""
      }`}
    >
      {it.what === "next" ? "↻ " : ""}
      {title}
    </div>
  );
}

export function MonthView(props: { today: string; selectedId: number | null; onOpen: (t: Task) => void }) {
  const { today } = props;
  const [month, setMonth] = useState(today.slice(0, 7)); // YYYY-MM
  const [picked, setPicked] = useState<string>(today);
  const [y, m] = month.split("-").map(Number);
  const from = mondayOf(`${month}-01`);
  const to = addDays(from, 41);
  const { data: items = [] } = useMonth(from, to);
  const byDay = new Map<string, CalItem[]>();
  for (const it of items) byDay.set(it.day, [...(byDay.get(it.day) ?? []), it]);
  const shift = (k: number) => setMonth(toYmd(new Date(y, m - 1 + k, 1)).slice(0, 7));
  const dayItems = byDay.get(picked) ?? [];

  return (
    <>
      <div className="flex shrink-0 items-center justify-between gap-4 pb-3">
        <h2 className="text-lg font-bold">
          {y} 年 {m} 月
        </h2>
        <div className="flex items-center gap-1">
          <IconButton label="上个月" onClick={() => shift(-1)}>
            <ChevronLeft size={16} />
          </IconButton>
          <Button
            variant="ghost"
            disabled={month === today.slice(0, 7)}
            onClick={() => {
              setMonth(today.slice(0, 7));
              setPicked(today);
            }}
          >
            本月
          </Button>
          <IconButton label="下个月" onClick={() => shift(1)}>
            <ChevronRight size={16} />
          </IconButton>
        </div>
      </div>
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pb-4">
        <div className="overflow-hidden rounded-card border bg-card">
          <div className="grid grid-cols-7 border-b text-center text-xs text-muted-foreground">
            {WEEK.map((w) => (
              <div key={w} className="py-1">
                {w}
              </div>
            ))}
          </div>
          <div className="grid grid-cols-7">
            {Array.from({ length: 42 }, (_, i) => addDays(from, i)).map((d, i) => {
              const list = byDay.get(d) ?? [];
              const inMonth = d.startsWith(month);
              return (
                <button
                  key={d}
                  type="button"
                  onClick={() => setPicked(d)}
                  aria-label={`${formatDay(d)}，${list.length} 项`}
                  className={`flex min-h-20 flex-col gap-0.5 border-b p-1 text-left outline-none transition-colors hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring ${
                    i % 7 !== 6 ? "border-r" : ""
                  } ${i >= 35 ? "border-b-0" : ""} ${d === picked ? "bg-accent" : ""}`}
                >
                  <span
                    className={`mb-0.5 inline-flex size-5 items-center justify-center self-start rounded-full text-xs ${
                      d === today ? "bg-primary text-primary-foreground" : inMonth ? "" : "text-muted-foreground/50"
                    }`}
                  >
                    {Number(d.slice(8))}
                  </span>
                  {list.slice(0, 3).map((it, k) => (
                    <Chip key={k} it={it} today={today} />
                  ))}
                  {list.length > 3 && <span className="px-1 text-[11px] text-muted-foreground">还有 {list.length - 3} 项</span>}
                </button>
              );
            })}
          </div>
        </div>
        <section>
          <h3 className="mb-1.5 px-1 text-sm text-muted-foreground">
            {formatDay(picked)} {weekday(picked)} · {dayItems.length} 项
          </h3>
          {dayItems.length > 0 ? (
            <PlacedList
              items={dayItems.map((it) => ({ ...it, detail: [WHAT[it.what], it.detail].filter(Boolean).join(" ") }))}
              today={today}
              selectedId={props.selectedId}
              onOpen={props.onOpen}
            />
          ) : (
            <p className="px-1 text-sm text-muted-foreground">这天没有安排</p>
          )}
        </section>
      </div>
    </>
  );
}
