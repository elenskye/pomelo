// 统计: a few numbers, the funnel (how far applications got — from `reached`, so rejected ones count
// up to where they stopped), applications per day / week (by 投递日期), and where everything is now.
import { useState, type ReactNode } from "react";
import { BarList, ColumnChart } from "@/shared/charts";
import { addDays, localToday, mondayOf } from "@/shared/dates";
import { Segmented } from "@/shared/ui";
import { useStatuses, type Job } from "./api";

type Span = "day" | "week";

function Kpi(props: { label: string; value: ReactNode }) {
  return (
    <div className="rounded-card border bg-card px-4 py-3">
      <p className="text-xs text-muted-foreground">{props.label}</p>
      <p className="text-xl font-bold">{props.value}</p>
    </div>
  );
}

function Card(props: { title: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="rounded-card border bg-card px-4 py-3">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm text-muted-foreground">{props.title}</h3>
        {props.actions}
      </div>
      {props.children}
    </section>
  );
}

export function StatsView(props: { jobs: Job[] }) {
  const { data: statuses = [] } = useStatuses();
  const [span, setSpan] = useState<Span>("week");
  const today = localToday();
  const jobs = props.jobs;
  const order = new Map(statuses.map((s) => [s.id, s]));
  const applied = jobs.filter((j) => j.appliedOn);
  const week0 = mondayOf(today);
  const fmt = (n: number) => `${n} 份`;

  // funnel: 已投递 → … → 拿到, each counting applications that reached at least that far
  const steps = statuses.filter((s) => s.stage === "applied" || s.stage === "active" || s.stage === "got");
  const furthest = (j: Job) => {
    const r = j.reachedId !== null ? order.get(j.reachedId) : undefined;
    const cur = order.get(j.statusId);
    return r ?? (cur && cur.stage !== "closed" ? cur : undefined);
  };
  const funnel = steps.map((s) => ({ s, n: jobs.filter((j) => (furthest(j)?.sortOrder ?? -1) >= s.sortOrder && furthest(j)?.stage !== "wish").length }));
  const top = funnel[0]?.n ?? 0;

  const buckets =
    span === "day"
      ? Array.from({ length: 30 }, (_, i) => addDays(today, i - 29)).map((d) => ({ key: d, from: d, to: d, label: `${Number(d.slice(8))}`, title: `${Number(d.slice(5, 7))} 月 ${Number(d.slice(8))} 日` }))
      : Array.from({ length: 12 }, (_, i) => addDays(week0, (i - 11) * 7)).map((d) => ({
          key: d,
          from: d,
          to: addDays(d, 6),
          label: `${Number(d.slice(5, 7))}/${Number(d.slice(8))}`,
          title: `${Number(d.slice(5, 7))} 月 ${Number(d.slice(8))} 日那一周`,
        }));
  const counts = buckets.map((b) => applied.filter((j) => j.appliedOn! >= b.from && j.appliedOn! <= b.to).length);

  return (
    <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pb-4">
      <div className="grid grid-cols-4 gap-3">
        <Kpi label="投出" value={fmt(applied.length)} />
        <Kpi label="本周投了" value={fmt(applied.filter((j) => j.appliedOn! >= week0).length)} />
        <Kpi label="进行中" value={fmt(jobs.filter((j) => j.stage === "applied" || j.stage === "active").length)} />
        <Kpi label="拿到" value={fmt(jobs.filter((j) => j.stage === "got").length)} />
      </div>
      <Card
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
      </Card>
      <div className="grid grid-cols-2 gap-4">
        <Card title="漏斗">
          {top === 0 ? (
            <p className="py-4 text-center text-sm text-muted-foreground">还没有投出去的</p>
          ) : (
            <BarList rows={funnel.map(({ s, n }) => ({ key: String(s.id), label: s.name, value: n, detail: `${Math.round((n / top) * 100)}%` }))} format={fmt} max={top} />
          )}
        </Card>
        <Card title="现在的状态">
          <BarList rows={statuses.map((s) => ({ key: String(s.id), label: s.name, value: s.count }))} format={fmt} />
        </Card>
      </div>
    </div>
  );
}
