import { useQuery } from "@tanstack/react-query";
import { useTabs } from "@/app/tabs";
import { AssistAction } from "@/modules/picky/Assist";
import { RATER_INTERVIEWS_KEY, interviewApi, sameJob } from "@/modules/picky/assistants/raterApi";
import type { Hit } from "@/shared/open";
import { Field } from "@/shared/ui";
import type { Job } from "./api";

/** The job's 面试 (Rater, v0.7 阶段 ⑦): its 准备 and 复盘, each opening Rater's 面试 page on this job;
 * 「准备面试」 and 「＋ 复盘」 open it with that dialog. */
export function JobInterviews(props: { job: Job; disabled: boolean }) {
  const tabs = useTabs();
  const all = useQuery({ queryKey: RATER_INTERVIEWS_KEY, queryFn: interviewApi.all });
  const go = (kind: "job" | "prep" | "review") => {
    const hit: Hit = { module: "rater", kind, id: props.job.id, title: "", detail: null, parent: null, day: null, amount: null, currency: null, flow: null };
    tabs.open("/rater/interview", { open: hit });
  };
  const preps = all.data?.preps.filter((p) => sameJob(p.job, props.job)) ?? [];
  const reviews = all.data?.reviews.filter((r) => sameJob(r.job, props.job)) ?? [];
  const rows = [...preps.map((p) => ({ id: p.id, at: p.createdAt, text: `准备 · ${p.createdAt.slice(0, 10)} · ${[p.lang, p.form].filter(Boolean).join(" · ")}` })), ...reviews.map((r) => ({ id: r.id, at: r.date, text: `复盘 · ${r.date} · ${[r.round, r.result].filter(Boolean).join(" · ")}` }))].sort((a, b) => b.at.localeCompare(a.at));
  return <Field label="面试" action={<span className="flex items-center gap-1"><AssistAction disabled={props.disabled} onClick={() => go("prep")}>准备面试</AssistAction><button type="button" disabled={props.disabled} className="rounded-control px-1 text-xs text-primary outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50" onClick={() => go("review")}>＋ 复盘</button></span>}>
    {rows.length ? <div className="space-y-1">{rows.map((r) => <button key={r.id} type="button" onClick={() => go("job")} className="block w-full truncate rounded-control border px-2 py-1 text-left text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring">{r.text}</button>)}</div> : <p className="text-sm text-muted-foreground">还没有</p>}
  </Field>;
}
