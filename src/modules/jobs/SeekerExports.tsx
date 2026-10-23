import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { SEEKER_KEY, seekerApi } from "@/modules/picky/assistants/seekerApi";
import { errorText, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { Dropdown } from "@/shared/Dropdown";
import { Button, Field } from "@/shared/ui";
import type { Job } from "./api";

export function JobSeekerExports(props: { job: Job; disabled: boolean }) {
  const qc = useQueryClient();
  const toast = useToast();
  const view = useQuery({ queryKey: SEEKER_KEY, queryFn: seekerApi.workspace });
  const [id, setId] = useState("");
  const [busy, setBusy] = useState(false);
  const output = view.data?.output;
  const links = output?.links.filter((l) => l.job.id === props.job.id && l.job.createdAt === props.job.createdAt) ?? [];
  const free = (output?.exports ?? []).filter((e) => !links.some((l) => l.exportId === e.id));
  const link = async (exportId: string) => {
    setBusy(true);
    try { qc.setQueryData(SEEKER_KEY, await seekerApi.jobLink(exportId, props.job.id)); setId(""); }
    catch (e) { toast(errorText(e), "error"); }
    finally { setBusy(false); }
  };
  return <Field label="Seeker 导出版本"><div className="space-y-2">
    {view.isError && <p className="text-sm text-destructive">{errorText(view.error)}</p>}
    {links.map((l) => { const item = output?.exports.find((e) => e.id === l.exportId); return <div key={l.id} className="space-y-2 rounded-control border p-2 text-sm"><p className="break-all">{item?.label ?? "导出版本缺失"}{l.current && <strong className="ml-2 text-primary">本次使用</strong>}</p><div className="flex flex-wrap gap-2"><Button disabled={!item} onClick={() => item && void ipc.openLink(item.path).catch((e) => toast(errorText(e), "error"))}>打开 Word</Button>{!l.current && <Button disabled={props.disabled || busy || !item} onClick={() => void link(l.exportId)}>用于本次投递</Button>}</div><details><summary>关联时的岗位与 JD</summary><p className="whitespace-pre-wrap pt-2">{[l.job.company, l.job.position, l.job.location, l.job.link, l.job.note, l.jd].filter(Boolean).join("\n")}</p></details></div>; })}
    {!view.isLoading && !view.isError && !links.length && !free.length && <p className="text-sm text-muted-foreground">还没有导出过</p>}
    {free.length > 0 && <div className="flex flex-wrap gap-2"><Dropdown ariaLabel="选择已导出的简历" value={id} placeholder="选择导出的简历" options={free.map((e) => ({ value: e.id, label: e.label }))} onChange={setId} /><Button disabled={props.disabled || busy || !id} onClick={() => void link(id)}>关联</Button></div>}
  </div></Field>;
}
