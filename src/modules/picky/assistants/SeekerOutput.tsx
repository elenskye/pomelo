import { PickField } from "@/shared/PickField";
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { FileText } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { Modal } from "@/shared/Modal";
import { Button, Field, TextInput } from "@/shared/ui";
import { errorText, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { useJobs, jobPicks } from "@/modules/jobs/api";
import { SEEKER_FILES_KEY, SEEKER_JOB_RESUMES_KEY, seekerApi, type SeekerForm, type SeekerJobRef, type SeekerLang, type SeekerOutput, type SeekerWorkspace } from "./seekerApi";
import { when } from "./SeekerParts";
import { useSeekerFiles } from "./SeekerFiles";

/** 另存 Word with Pomelo's built-in template of the language (Rust output.rs): no page count (Word
 * opens it right after, to see the pages). Default folder cv/cn/ or cv/en/, default job the draft's. */
export function SeekerExportDialog(props: { lang: SeekerLang; form: SeekerForm; name: string; folder: string; job: SeekerJobRef | null | undefined; flush: () => Promise<boolean>; saved: (workspace: SeekerWorkspace) => void; close: () => void }) {
  const toast = useToast();
  const jobs = useJobs();
  const [directory, setDirectory] = useState(() => (props.folder ? `${props.folder}/cv/${props.lang}` : ""));
  const [filename, setFilename] = useState(() => `${props.name || "简历"}${props.lang === "en" ? (props.form === "cv" ? "-CV" : "-Resume") : ""}-${new Date().toISOString().slice(0, 10).replace(/-/g, "")}.docx`);
  const own = jobs.data?.find((j) => props.job && j.id === props.job.id && j.createdAt === props.job.createdAt)?.id ?? null;
  const [picked, setPicked] = useState<number | null | undefined>(undefined);
  const jobId = picked === undefined ? own : picked;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const qc = useQueryClient();
  const save = async () => {
    if (busy) return;
    setBusy(true); setError(null);
    try {
      if (!await props.flush()) { setError("请先保存当前草稿"); return; }
      const result = await seekerApi.outputSave(directory, filename, jobId, props.lang);
      props.saved(result.workspace); toast(`已导出：${result.export.path}`); props.close();
      void qc.invalidateQueries({ queryKey: SEEKER_FILES_KEY }); void qc.invalidateQueries({ queryKey: SEEKER_JOB_RESUMES_KEY });
      void ipc.openLink(result.export.path).catch((e) => toast(errorText(e), "error"));
    } catch (e) { setError(errorText(e)); setBusy(false); }
  };
  return <Modal wide title="导出 Word" error={error} onClose={() => { if (!busy) props.close(); }} submitLabel={busy ? "正在导出…" : "另存 Word"} submitDisabled={busy || !directory.trim() || !filename.trim()} onSubmit={(e) => { e.preventDefault(); void save(); }}>
    <fieldset disabled={busy} className="space-y-4">
      <Field label="导出到"><div className="flex items-center gap-2"><p className="selectable min-w-0 flex-1 truncate text-sm" title={directory}>{directory.replace(/^\/Users\/[^/]+/, "~") || "还没选文件夹"}</p><Button onClick={() => void open({ directory: true, multiple: false, defaultPath: directory || undefined }).then((p) => { if (typeof p === "string") setDirectory(p); }).catch((e) => setError(errorText(e)))}>选择…</Button></div></Field>
      <Field label="文件名"><TextInput aria-label="文件名" value={filename} onChange={(e) => setFilename(e.target.value)} /></Field>
      <Field label="关联投石问路"><PickField<number | null> block ariaLabel="关联投石问路" title="关联投石问路的岗位" value={jobId} options={[{ value: null, label: "暂不关联" }, ...jobPicks(jobs.data ?? [])]} onChange={setPicked} /></Field>
      {jobs.isError && <p className="text-xs text-destructive">{errorText(jobs.error)}</p>}
    </fieldset>
  </Modal>;
}

/** 历史 · Word 导出: what was exported in this language and is still under cv/ (newest first); a click
 * opens the Word. Seeker keeps no copy of the résumé itself. */
export function SeekerExports(props: { lang: SeekerLang; output: SeekerOutput }) {
  const toast = useToast();
  const files = useSeekerFiles();
  const present = new Set((files.data?.cv ?? []).map((f) => f.exportId).filter(Boolean));
  const exports = props.output.exports.filter((e) => (e.lang ?? "cn") === props.lang && present.has(e.id));
  if (!files.data) return null;
  return <section className="space-y-2">
    <h2 className="text-sm font-bold">Word 导出</h2>
    {exports.length ? <div className="rounded-card border bg-card p-1">{exports.map((e) => <button key={e.id} type="button" onClick={() => void ipc.openLink(e.path).catch((err) => toast(errorText(err), "error"))} title={e.path} className="flex w-full items-center gap-2 rounded-control px-2 py-1.5 text-left text-sm outline-none hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring">
      <FileText size={14} className="shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1 truncate">{e.label}</span>
      <span className="shrink-0 text-xs text-muted-foreground">{when(e.createdAt)}</span>
    </button>)}</div> : <p className="text-sm text-muted-foreground">cv/ 里没有这种语言的 Word 导出</p>}
  </section>;
}
