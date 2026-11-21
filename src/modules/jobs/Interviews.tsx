import { useQuery } from "@tanstack/react-query";
import { MessageSquare, Sparkles } from "@/shared/icons";
import { useTabs } from "@/app/tabs";
import { RATER_INTERVIEWS_KEY, interviewApi, sameJob } from "@/modules/youzi/assistants/raterApi";
import type { Hit } from "@/shared/open";
import { Button, Field } from "@/shared/ui";
import type { Job } from "./api";

/** The job's 面试 (Rater, v0.7 阶段 ⑦): 「准备面试」「复盘」 (two wide buttons) open Rater's 面试 page on
 * this job with that dialog (复盘: the start of a record's conversation); its 准备, 复盘 and 总结 below, each
 * opening the page. */
export function JobInterviews(props: { job: Job; disabled: boolean }) {
  const tabs = useTabs();
  const all = useQuery({ queryKey: RATER_INTERVIEWS_KEY, queryFn: interviewApi.all });
  const go = (kind: "job" | "prep" | "review") => {
    const hit: Hit = { module: "rater", kind, id: props.job.id, title: "", detail: null, parent: null, day: null, amount: null, currency: null, flow: null };
    tabs.open("/rater/interview", { open: hit });
  };
  const preps = all.data?.preps.filter((p) => sameJob(p.job, props.job)) ?? [];
  const reviews = all.data?.reviews.filter((r) => sameJob(r.job, props.job)) ?? [];
  const summaries = all.data?.summaries.filter((s) => sameJob(s.job, props.job)) ?? [];
  const rows = [...preps.map((p) => ({ id: p.id, at: p.createdAt, text: `准备 · ${p.createdAt.slice(0, 10)} · ${[p.lang, p.form].filter(Boolean).join(" · ")}` })), ...reviews.map((r) => ({ id: r.id, at: r.date, text: `复盘 · ${r.date} · ${[r.round, r.result].filter(Boolean).join(" · ")}` })), ...summaries.map((s) => ({ id: s.id, at: s.createdAt, text: `总结 · ${s.createdAt.slice(0, 10)} · ${s.reviews.length} 次复盘` }))].sort((a, b) => b.at.localeCompare(a.at));
  return <Field label="面试"><div className="space-y-2">
    <div className="grid grid-cols-2 gap-2">
      <Button className="justify-center" disabled={props.disabled} onClick={() => go("prep")}><Sparkles size={14} strokeWidth={1.75} />准备面试</Button>
      <Button className="justify-center" disabled={props.disabled} onClick={() => go("review")}><MessageSquare size={14} strokeWidth={1.75} />复盘</Button>
    </div>
    {rows.length > 0 && <div className="space-y-1">{rows.map((r) => <button key={r.id} type="button" onClick={() => go("job")} className="block w-full truncate rounded-control border px-2 py-1 text-left text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring">{r.text}</button>)}</div>}
  </div></Field>;
}
