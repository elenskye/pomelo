// cv/ and material/ of the Seeker folder (Rust seeker/files.rs): read in place, never changed. 历史 shows
// cv/'s resumes by series (their versions newest first: 打开, 在访达中显示, 正文, 比较上一版); 网申 lists
// material/'s files by name (打开, 在访达中显示) — Pomelo doesn't read them.
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { FileText, FolderOpen } from "lucide-react";
import { errorText, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { Button, EmptyState, IconButton } from "@/shared/ui";
import { SeekerDiff } from "./SeekerParts";
import { SEEKER_FILES_KEY, seekerApi, type SeekerCvFile, type SeekerLang } from "./seekerApi";

export const folderOf = (path: string) => path.slice(0, path.lastIndexOf("/")) || "/";
const size = (n: number) => (n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`);
export const cvDate = (secs: number) => { const d = new Date(secs * 1000); const p = (n: number) => String(n).padStart(2, "0"); return `${d.getFullYear()} / ${p(d.getMonth() + 1)} / ${p(d.getDate())}`; };
export const useSeekerFiles = () => useQuery({ queryKey: SEEKER_FILES_KEY, queryFn: seekerApi.files });

function useOpen() {
  const toast = useToast();
  return (path: string) => void ipc.openLink(path).catch((e) => toast(errorText(e), "error"));
}

/** 历史 → cv/ 里的简历: this language's, by series. */
export function SeekerCvHistory(props: { lang: SeekerLang }) {
  const files = useSeekerFiles();
  const open = useOpen();
  const [shown, setShown] = useState<{ path: string; text: string; before?: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  if (files.isError) return <p className="text-sm text-destructive">{errorText(files.error)}</p>;
  if (!files.data) return null;
  if (files.data.error) return <p className="text-sm text-destructive">{files.data.error}</p>;
  const list = files.data.cv.filter((f) => f.lang === props.lang);
  if (!list.length) return <EmptyState title="cv/ 里没有这种语言的简历" />;
  const series = [...new Set(list.map((f) => f.series))];
  const read = async (f: SeekerCvFile, older?: SeekerCvFile) => {
    setError(null);
    try {
      const text = await seekerApi.cvText(f.path);
      setShown({ path: f.path, text, before: older ? await seekerApi.cvText(older.path) : undefined });
    } catch (e) { setError(errorText(e)); }
  };
  return <div className="space-y-4">
    {error && <p className="text-sm text-destructive">{error}</p>}
    {series.map((s) => {
      const versions = list.filter((f) => f.series === s);
      const jobs = [...new Set(versions.flatMap((f) => f.jobs))];
      return <section key={s} className="space-y-1">
        <h4 className="text-sm font-bold">{s}<span className="ml-2 font-normal text-muted-foreground">{versions.length > 1 ? `${versions.length} 版` : ""}{jobs.length > 0 && `${versions.length > 1 ? " · " : ""}投石问路：${jobs.join("、")}`}</span></h4>
        <div className="rounded-card border bg-card p-1">{versions.map((f, i) => {
          const older = versions[i + 1];
          const readable = /\.(docx|pdf)$/i.test(f.name);
          return <div key={f.path}>
            <div className="group flex items-center gap-2 rounded-control px-2 py-1.5 text-sm hover:bg-muted/60">
              <FileText size={14} className="shrink-0 text-muted-foreground" />
              <span className="min-w-0 flex-1 truncate">{f.name.split("/").pop()}{f.template && <span className="ml-2 text-xs text-primary">模板</span>}{f.export && <span className="ml-2 text-xs text-muted-foreground">Pomelo 导出</span>}</span>
              <span className="shrink-0 text-xs text-muted-foreground">{cvDate(f.modified)} · {size(f.size)}</span>
              <span className="flex shrink-0 items-center gap-1">
                {readable && <Button variant="ghost" onClick={() => (shown?.path === f.path && !shown.before ? setShown(null) : void read(f))}>正文</Button>}
                {readable && older && /\.(docx|pdf)$/i.test(older.name) && <Button variant="ghost" onClick={() => (shown?.path === f.path && shown.before != null ? setShown(null) : void read(f, older))}>比较上一版</Button>}
                <Button variant="ghost" onClick={() => open(f.path)}>打开</Button>
                <IconButton label="在访达中显示" onClick={() => open(folderOf(f.path))}><FolderOpen size={14} /></IconButton>
              </span>
            </div>
            {shown?.path === f.path && <div className="px-2 pb-2">{shown.before != null ? <SeekerDiff before={shown.before} after={shown.text} /> : <pre className="max-h-[45vh] overflow-auto whitespace-pre-wrap rounded-control border p-3 font-sans text-sm">{shown.text || "（没有文字）"}</pre>}</div>}
          </div>;
        })}</div>
      </section>;
    })}
  </div>;
}

/** 网申 → 证书原件: material/'s files by sub-folder, names only. */
export function SeekerMaterialFiles() {
  const files = useSeekerFiles();
  const open = useOpen();
  const list = files.data?.material ?? [];
  if (!list.length) return null;
  const groups = [...new Set(list.map((f) => f.group))];
  return <section className="space-y-2">
    <h3 className="text-sm font-bold">证书原件<span className="ml-2 font-normal text-muted-foreground">material/ · {list.length}</span></h3>
    {groups.map((g) => <div key={g} className="space-y-1">
      {g && <p className="px-1 text-xs text-muted-foreground">{g}</p>}
      <div className="rounded-card border bg-card p-1">{list.filter((f) => f.group === g).map((f) => <div key={f.path} className="group flex items-center gap-2 rounded-control px-2 py-1 text-sm hover:bg-muted/60">
        <button type="button" onClick={() => open(f.path)} className="min-w-0 flex-1 truncate rounded-control text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">{f.name}</button>
        <span className="shrink-0 text-xs text-muted-foreground">{size(f.size)}</span>
        <span className="opacity-0 transition-opacity duration-150 group-hover:opacity-100 focus-within:opacity-100"><IconButton label="在访达中显示" onClick={() => open(folderOf(f.path))}><FolderOpen size={14} /></IconButton></span>
      </div>)}</div>
    </div>)}
  </section>;
}
