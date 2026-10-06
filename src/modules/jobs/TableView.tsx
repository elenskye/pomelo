// 投递: 该跟进 reminders on top, then the table (filter by stage group, search company / 岗位 / 地点).
import { useState } from "react";
import { BellRing } from "lucide-react";
import { useKinds } from "@/shared/kinds/api";
import { Button, SearchInput, Segmented } from "@/shared/ui";
import { localToday } from "@/shared/dates";
import { CHANNELS, jobLabel, STAGE_STYLE, useJobMutations, useStatuses, type Job, type Stage } from "./api";

type Filter = "all" | "wish" | "open" | "got" | "closed";
const FILTERS: { value: Filter; label: string; stages: Stage[] }[] = [
  { value: "all", label: "全部", stages: ["wish", "applied", "active", "got", "closed"] },
  { value: "wish", label: "想投", stages: ["wish"] },
  { value: "open", label: "进行中", stages: ["applied", "active"] },
  { value: "got", label: "拿到", stages: ["got"] },
  { value: "closed", label: "已结束", stages: ["closed"] },
];
const slash = (d: string) => d.split("-").join(" / ");

/** What comes next, by stage: deadline · waiting / 该跟进 · step date. */
function Next({ j, today }: { j: Job; today: string }) {
  if (j.stage === "wish" && j.deadline) return <span className={j.deadline < today ? "text-destructive" : undefined}>截止 {slash(j.deadline)}</span>;
  if (j.stage === "applied" && j.followUp) return <span className="text-[var(--status-doing-fg)]">该跟进</span>;
  if (j.stage === "applied" && j.waitingDays !== null) return <span className="text-muted-foreground">等了 {j.waitingDays} 天</span>;
  if (j.stage === "active" && j.stepOn) return <span className={j.stepOn < today ? "text-destructive" : undefined}>{slash(j.stepOn)}</span>;
  return <span className="text-muted-foreground">—</span>;
}

export function TableView(props: { jobs: Job[]; selectedId: number | null; onOpen: (j: Job) => void }) {
  const { data: statuses = [] } = useStatuses();
  const { data: channels = [] } = useKinds(CHANNELS);
  const m = useJobMutations();
  const [filter, setFilter] = useState<Filter>("all");
  const [q, setQ] = useState("");
  const today = localToday();
  const statusName = new Map(statuses.map((s) => [s.id, s.name]));
  const channelName = new Map(channels.map((k) => [k.id, k.name]));
  const due = props.jobs.filter((j) => j.followUp);
  const stages = FILTERS.find((f) => f.value === filter)!.stages;
  const needle = q.trim().toLowerCase();
  const shown = props.jobs.filter(
    (j) => stages.includes(j.stage) && (!needle || [j.company, j.position, j.location].some((s) => s?.toLowerCase().includes(needle))),
  );
  const count = (f: (typeof FILTERS)[number]) => props.jobs.filter((j) => f.stages.includes(j.stage)).length;

  return (
    <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pb-4">
      {due.length > 0 && (
        <section className="rounded-card border bg-card">
          <h3 className="flex items-center gap-1.5 border-b px-4 py-2 text-sm">
            <BellRing size={14} className="text-[var(--status-doing-fg)]" />
            该跟进 {due.length}
          </h3>
          <ul>
            {due.map((j) => (
              <li key={j.id} className="flex items-center gap-3 border-b px-4 py-1.5 text-sm last:border-b-0">
                <button type="button" onClick={() => props.onOpen(j)} className="min-w-0 flex-1 truncate text-left hover:underline">
                  {jobLabel(j)}
                </button>
                <span className="shrink-0 text-xs text-muted-foreground">
                  {j.followedOn ? "跟进后" : "投了"} {j.waitingDays} 天
                </span>
                <Button variant="ghost" className="py-1" onClick={() => m.followUp.mutate(j.id)}>
                  已跟进
                </Button>
              </li>
            ))}
          </ul>
        </section>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <Segmented ariaLabel="筛选" value={filter} onChange={setFilter} options={FILTERS.map((f) => ({ value: f.value, label: `${f.label} ${count(f)}` }))} />
        <div className="w-56">
          <SearchInput value={q} onChange={(e) => setQ(e.target.value)} placeholder="搜索公司、岗位、地点" aria-label="搜索" />
        </div>
      </div>
      {shown.length === 0 ? (
        <p className="py-10 text-center text-muted-foreground">{props.jobs.length === 0 ? "还没有投递" : "没有符合的投递"}</p>
      ) : (
        <table className="w-full overflow-hidden rounded-card border bg-card text-sm">
          <thead className="border-b text-xs text-muted-foreground">
            <tr>
              <th className="px-3 py-2 text-left font-normal">公司</th>
              <th className="px-3 py-2 text-left font-normal">岗位</th>
              <th className="w-24 px-3 py-2 text-left font-normal">地点</th>
              <th className="w-32 px-3 py-2 text-left font-normal">状态</th>
              <th className="w-32 px-3 py-2 text-left font-normal">投递日期</th>
              <th className="w-40 px-3 py-2 text-left font-normal">下一步</th>
              <th className="w-24 px-3 py-2 text-left font-normal">渠道</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((j) => (
              <tr
                key={j.id}
                onClick={() => props.onOpen(j)}
                className={`cursor-pointer border-b transition-colors duration-150 last:border-b-0 ${props.selectedId === j.id ? "bg-accent" : "hover:bg-muted/70"} ${
                  j.stage === "closed" ? "text-muted-foreground" : ""
                }`}
              >
                <td className="max-w-0 truncate px-3 py-2">{j.company}</td>
                <td className="max-w-0 truncate px-3 py-2">{j.position}</td>
                <td className="truncate px-3 py-2">{j.location}</td>
                <td className="px-3 py-2">
                  <span className="rounded px-1.5 py-0.5 text-xs whitespace-nowrap" style={{ background: STAGE_STYLE[j.stage].bg, color: STAGE_STYLE[j.stage].fg }}>
                    {statusName.get(j.statusId)}
                  </span>
                </td>
                <td className="px-3 py-2 tabular-nums">{j.appliedOn ? slash(j.appliedOn) : ""}</td>
                <td className="px-3 py-2 whitespace-nowrap tabular-nums">
                  <Next j={j} today={today} />
                </td>
                <td className="truncate px-3 py-2 text-muted-foreground">{j.kindId !== null ? channelName.get(j.kindId) : ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
