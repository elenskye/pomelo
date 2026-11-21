// 统计: a few numbers, the funnel (per step: checked / applications that have the step — one that
// skipped 笔试 counts for neither), applications per day / week (by 投递日期 = the first checked day),
// and where everything is now. All computed in Rust (jobs.rs `stats`), shared with 首页 and 柚子.
import { useState, type ReactNode } from "react";
import { BarList, ChartCard, ColumnChart } from "@/shared/charts";
import { Segmented } from "@/shared/ui";
import { useJobStats } from "./api";

type Span = "day" | "week";

function Kpi(props: { label: string; value: ReactNode }) {
  return (
    <div className="rounded-card border bg-card px-4 py-3">
      <p className="text-xs text-muted-foreground">{props.label}</p>
      <p className="text-xl font-bold">{props.value}</p>
    </div>
  );
}

export function StatsView() {
  const { data: s } = useJobStats();
  const [span, setSpan] = useState<Span>("week");
  if (!s) return null;
  const fmt = (n: number) => `${n} 份`;
  const top = Math.max(0, ...s.funnel.map((f) => f.have));
  const day = (d: string) => `${Number(d.slice(5, 7))} 月 ${Number(d.slice(8))} 日`;
  const buckets =
    span === "day"
      ? s.days.map((b) => ({ ...b, label: `${Number(b.from.slice(8))}`, title: day(b.from) }))
      : s.weeks.map((b) => ({ ...b, label: `${Number(b.from.slice(5, 7))}/${Number(b.from.slice(8))}`, title: `${day(b.from)}那一周` }));
  const counts = buckets.map((b) => b.count);

  return (
    <div className="-m-1 min-h-0 flex-1 space-y-4 overflow-y-auto p-1 pb-4">
      <div className="grid grid-cols-4 gap-3">
        <Kpi label="投出" value={fmt(s.applied)} />
        <Kpi label="本周投了" value={fmt(s.thisWeek)} />
        <Kpi label="进行中" value={fmt(s.ongoing)} />
        <Kpi label="拿到" value={fmt(s.got)} />
      </div>
      <ChartCard
        title={span === "day" ? "最近 30 天每天投了几份" : "最近 12 周每周投了几份"}
        actions={
          <Segmented
            ariaLabel="按天还是按周"
            value={span}
            onChange={setSpan}
            options={[
              { value: "day" as Span, label: "每天" },
              { value: "week" as Span, label: "每周" },
            ]}
          />
        }
      >
        <div className="pt-6">
          <ColumnChart
            labels={buckets.map((b) => b.label)}
            series={[{ name: "投递", color: "var(--chart-1)" }]}
            values={[counts]}
            format={fmt}
            axisFormat={String}
            integer
            labelEvery={span === "day" ? 5 : 1}
            title={(i) => buckets[i].title}
          />
        </div>
      </ChartCard>
      <div className="grid grid-cols-2 gap-4">
        <ChartCard title="漏斗">
          {top === 0 ? (
            <p className="py-4 text-center text-sm text-muted-foreground">还没有投出去的</p>
          ) : (
            <BarList rows={s.funnel.map((f) => ({ key: String(f.statusId), label: f.name, value: f.done, detail: `/ ${f.have} 份` }))} format={String} max={top} />
          )}
        </ChartCard>
        <ChartCard title="现在的状态">
          <BarList rows={s.statuses.map((x) => ({ key: String(x.id), label: x.name, value: x.count }))} format={fmt} />
        </ChartCard>
      </div>
    </div>
  );
}
