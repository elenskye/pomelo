// 发布页: what would go online (click a row for its diff), 发布… (fetch first, then the confirm dialog with
// the commit text), the running publish's steps, 最近发布. Rules (size limits, fast-forward only, the
// check, what gets committed) are Rust's.
import { useEffect, useRef, useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, X } from "lucide-react";
import { errorText, useToast } from "@/shared/feedback";
import { api } from "@/shared/ipc";
import { Modal } from "@/shared/Modal";
import { Button, Field, TextArea } from "@/shared/ui";
import { ACTION_LABEL, OVERVIEW_KEY, SITE_KEY, siteApi, sizeText, useChanges, whenText, type Change, type Changes, type Diff, type History, type Job, type Overview, type Published } from "./api";
import { watchJob } from "@/app/Pomelo";

const rowClass = "flex w-full items-baseline gap-3 border-b px-4 py-2.5 text-left text-sm last:border-b-0";

function ChangeLabel(props: { c: Change }) {
  const c = props.c;
  return (
    <>
      <span className={`w-8 shrink-0 text-xs ${c.blocked ? "text-destructive" : "text-muted-foreground"}`}>{ACTION_LABEL[c.action]}</span>
      <span className={`min-w-0 flex-1 ${c.blocked ? "text-destructive" : ""}`}>
        <span className={`block truncate ${c.kind === "code" ? "font-mono text-xs" : ""}`}>{c.label}</span>
        {c.blocked && <span className="block text-xs">{c.blocked}</span>}
      </span>
      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{sizeText(c.size)}</span>
    </>
  );
}

function DiffView(props: { path: string }) {
  const { data, error } = useQuery({ queryKey: [...SITE_KEY, "diff", props.path], queryFn: () => siteApi.diff(props.path), staleTime: 0, gcTime: 0 });
  if (error) return <p className="px-4 py-3 text-sm text-destructive">{errorText(error)}</p>;
  if (!data) return <p className="px-4 py-3 text-xs text-muted-foreground">正在读取…</p>;
  return <DiffBody d={data} />;
}

function DiffBody(props: { d: Diff }) {
  const d = props.d;
  if (d.type === "image")
    return (
      <div className="grid grid-cols-2 gap-4 px-4 py-3">
        {(["以前", "现在"] as const).map((label, i) => {
          const src = i === 0 ? d.before : d.after;
          return (
            <figure key={label} className="space-y-1.5">
              <figcaption className="text-xs text-muted-foreground">{label}</figcaption>
              {src ? <img src={src} alt={label} className="max-h-60 rounded-control border object-contain" /> : <div className="flex h-24 items-center justify-center rounded-control border border-dashed text-xs text-muted-foreground">没有</div>}
            </figure>
          );
        })}
      </div>
    );
  if (d.type === "binary")
    return (
      <p className="px-4 py-3 text-xs text-muted-foreground tabular-nums">
        以前 {d.before === null ? "没有" : sizeText(d.before)} → 现在 {d.after === null ? "没有" : sizeText(d.after)}
      </p>
    );
  return (
    <div className="-m-1 max-h-96 overflow-auto p-1">
      <div className="min-w-fit px-4 py-2 font-mono text-xs leading-relaxed">
        {d.lines.map((l, i) =>
          l.t === "hunk" ? (
            <div key={i} className="py-1 text-muted-foreground">
              ⋯ {l.s}
            </div>
          ) : (
            <div key={i} className={`whitespace-pre-wrap break-words ${l.t === "del" ? "text-muted-foreground line-through" : l.t === "add" ? "rounded-sm bg-accent" : ""}`}>
              {l.s || " "}
            </div>
          ),
        )}
        {d.truncated && <div className="py-1 text-muted-foreground">⋯</div>}
      </div>
    </div>
  );
}

function ChangeRows(props: { files: Change[] }) {
  const [open, setOpen] = useState<string | null>(null);
  return (
    <div className="overflow-hidden rounded-card border bg-card">
      {props.files.map((c) => (
        <div key={c.path} className="border-b last:border-b-0">
          <button
            type="button"
            aria-expanded={open === c.path}
            onClick={() => setOpen(open === c.path ? null : c.path)}
            className={`${rowClass} border-b-0 transition-colors duration-150 outline-none focus-visible:bg-muted/70 ${open === c.path ? "bg-muted/50" : "hover:bg-muted/70"}`}
          >
            <ChangeLabel c={c} />
          </button>
          {open === c.path && (
            <div className="border-t">
              <DiffView path={c.path} />
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function CommitRows(props: { commits: { sha: string; subject: string; time: number }[] }) {
  return (
    <div className="overflow-hidden rounded-card border bg-card">
      {props.commits.map((c) => (
        <div key={c.sha} className={rowClass}>
          <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{whenText(c.time)}</span>
          <span className="min-w-0 flex-1 truncate">{c.subject}</span>
        </div>
      ))}
    </div>
  );
}

function PublishDialog(props: { changes: Changes; onClose: () => void; onStarted: (o: Overview) => void }) {
  const { files, ahead, token } = props.changes;
  const [message, setMessage] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const textRef = useRef<HTMLTextAreaElement>(null);
  // once the dialog has taken focus on opening
  useEffect(() => {
    const t = setTimeout(() => textRef.current?.focus(), 0);
    return () => clearTimeout(t);
  }, []);
  const blocked = files.some((c) => c.blocked);
  const needsText = files.length > 0 && !message.trim();
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (blocked || needsText || sending) return;
    setSending(true);
    try {
      props.onStarted(await siteApi.publish(message, token));
      // the pomelo says it if it fails after this page is left
      watchJob("site", () =>
        siteApi.overview().then((o) => (!o.job || o.job.state === "running" ? null : o.job.state === "failed" ? `${o.job.step === "deploy" ? "网站没上线" : "发布没成功"}：${o.job.error ?? "看呦呦漫游的发布页"}` : "")),
      );
    } catch (err) {
      setError(errorText(err));
      setSending(false);
    }
  };
  return (
    <Modal title="发布" wide onClose={props.onClose} onSubmit={(e) => void submit(e)} error={error} submitLabel="发布" submitDisabled={blocked || needsText || sending}>
      <div tabIndex={-1} className="max-h-[45vh] overflow-y-auto rounded-card border outline-none">
        {files.map((c) => (
          <div key={c.path} className={`${rowClass} py-2`}>
            <ChangeLabel c={c} />
          </div>
        ))}
        {ahead.length > 0 && <p className="border-b bg-muted/40 px-4 py-1.5 text-xs text-muted-foreground">还没推送的提交 {ahead.length}</p>}
        {ahead.map((c) => (
          <div key={c.sha} className={`${rowClass} py-2`}>
            <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{whenText(c.time)}</span>
            <span className="min-w-0 flex-1 truncate">{c.subject}</span>
          </div>
        ))}
      </div>
      {files.length > 0 && (
        <Field label="commit 文本">
          <TextArea ref={textRef} rows={3} value={message} onChange={(e) => setMessage(e.target.value)} className="font-mono text-xs" />
        </Field>
      )}
    </Modal>
  );
}

const STEPS: { key: "pull" | Job["step"]; label: string }[] = [
  { key: "pull", label: "拉取" },
  { key: "check", label: "检查" },
  { key: "push", label: "提交并推送" },
  { key: "deploy", label: "上线" },
];

function useSeconds(since: number, on: boolean) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!on) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [on]);
  return Math.max(0, Math.round((now - since) / 1000));
}

function JobCard(props: { job: Job; siteUrl: string | null; onDismiss: () => void }) {
  const j = props.job;
  const at = STEPS.findIndex((s) => s.key === j.step);
  const secs = useSeconds(j.since, j.state === "running");
  const logRef = useRef<HTMLPreElement>(null);
  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [j.log.length, j.state]);
  const open = (url: string) => void api.openLink(url);
  return (
    <div className="rounded-card border bg-card px-4 py-3">
      <div className="mb-2 flex min-h-7 items-center justify-between gap-4">
        <p className="min-w-0 truncate text-sm font-bold">{j.subject || "推送还没推的提交"}</p>
        {j.state !== "running" && (
          <Button variant="ghost" className="-mr-2 text-muted-foreground" onClick={props.onDismiss}>
            关闭
          </Button>
        )}
      </div>
      <ol className="space-y-1.5 text-sm">
        {STEPS.map((s, i) => {
          const done = i < at || (i === at && j.state === "done");
          const failed = i === at && j.state === "failed";
          const running = i === at && j.state === "running";
          return (
            <li key={s.key} className={`flex items-center gap-2 ${i > at ? "text-muted-foreground" : ""}`}>
              <span className="flex size-4 shrink-0 items-center justify-center">
                {done ? <Check size={14} className="text-[var(--status-done-fg)]" /> : failed ? <X size={14} className="text-destructive" /> : running ? <span className="size-1.5 animate-pulse rounded-full bg-[var(--status-doing-fg)]" /> : <span className="size-1.5 rounded-full bg-muted-foreground/30" />}
              </span>
              <span className="shrink-0">{s.label}</span>
              {running && (
                <span className="min-w-0 truncate text-xs text-muted-foreground tabular-nums">
                  已用 {secs} 秒{s.key === "check" && j.log[0] ? ` · ${j.log[0]}` : ""}
                </span>
              )}
            </li>
          );
        })}
      </ol>
      {j.state === "failed" && (
        <div className="mt-3 space-y-2">
          <p className="text-sm text-destructive">{j.error}</p>
          {j.log.length > 0 && (
            <pre ref={logRef} className="max-h-56 overflow-auto rounded-control border bg-muted/40 px-3 py-2 font-mono text-xs leading-relaxed whitespace-pre-wrap break-words">
              {j.log.join("\n")}
            </pre>
          )}
          {j.deployUrl && <Button onClick={() => open(j.deployUrl!)}>看 Vercel 的记录</Button>}
        </div>
      )}
      {j.state === "done" && (
        <div className="mt-3 flex items-center gap-3">
          {j.unknown ? <p className="text-sm text-muted-foreground">已推送，没等到 Vercel 的结果</p> : <p className="text-sm text-[var(--status-done-fg)]">已上线</p>}
          {props.siteUrl && <Button onClick={() => open(props.siteUrl!)}>打开网站</Button>}
          {j.unknown && j.deployUrl && <Button variant="ghost" onClick={() => open(j.deployUrl!)}>Vercel</Button>}
        </div>
      )}
    </div>
  );
}

const STATE: Record<string, { label: string; cls: string }> = {
  success: { label: "已上线", cls: "text-[var(--status-done-fg)]" },
  pending: { label: "上线中", cls: "text-[var(--status-doing-fg)]" },
  failure: { label: "没上线", cls: "text-destructive" },
  error: { label: "没上线", cls: "text-destructive" },
};

function HistoryRows(props: { items: Published[]; githubUrl: string | null }) {
  const open = (p: Published) => {
    const url = p.url ?? (props.githubUrl ? `${props.githubUrl}/commit/${p.sha}` : null);
    if (url) void api.openLink(url);
  };
  return (
    <div className="overflow-hidden rounded-card border bg-card">
      {props.items.map((p) => {
        const st = p.state ? STATE[p.state] : null;
        return (
          <button key={p.sha} type="button" onClick={() => open(p)} className={`${rowClass} transition-colors duration-150 outline-none hover:bg-muted/70 focus-visible:bg-muted/70`}>
            <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{whenText(p.time)}</span>
            <span className="min-w-0 flex-1 truncate">{p.subject}</span>
            {st && <span className={`shrink-0 text-xs ${st.cls}`}>{st.label}</span>}
          </button>
        );
      })}
    </div>
  );
}

export function PublishPage(props: { o: Overview; history: History | null }) {
  const { o } = props;
  const qc = useQueryClient();
  const toast = useToast();
  const { data: changes } = useChanges(true);
  const [preparing, setPreparing] = useState(false);
  const [confirm, setConfirm] = useState<Changes | null>(null);
  const job = o.job;
  const running = job?.state === "running";
  // a publish that ends changes both lists
  const ended = job && job.state !== "running" ? `${job.id}:${job.state}` : null;
  useEffect(() => {
    if (ended) void qc.invalidateQueries({ queryKey: SITE_KEY, predicate: (q) => q.queryKey[1] !== "overview" });
  }, [ended, qc]);
  const prepare = async () => {
    setPreparing(true);
    try {
      const c = await siteApi.prepare();
      qc.setQueryData([...SITE_KEY, "changes"], c);
      if (c.files.length === 0 && c.ahead.length === 0) toast("网站和线上一样");
      else setConfirm(c);
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setPreparing(false);
    }
  };
  const dismiss = () => void siteApi.dismiss().then((v) => qc.setQueryData(OVERVIEW_KEY, v));
  const files = changes?.files ?? [];
  const ahead = changes?.ahead ?? [];
  const count = files.length + ahead.length;
  return (
    <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
      <div className="mx-auto max-w-3xl space-y-6 px-8 pt-1 pb-8">
        {job && <JobCard job={job} siteUrl={o.siteUrl} onDismiss={dismiss} />}
        <section className="space-y-2">
          <div className="flex items-end justify-between gap-4">
            <h2 className="text-sm text-muted-foreground">
              待发布的改动 {count > 0 && <span className="text-foreground">{count}</span>}
            </h2>
            <Button variant="primary" disabled={preparing || running} onClick={() => void prepare()}>
              {preparing ? "正在拉取…" : "发布…"}
            </Button>
          </div>
          {changes && count === 0 && <p className="rounded-card border bg-card px-4 py-3 text-sm text-muted-foreground">网站和线上一样</p>}
          {files.length > 0 && <ChangeRows files={files} />}
          {ahead.length > 0 && (
            <div className="space-y-1.5 pt-2">
              <p className="text-xs text-muted-foreground">还没推送的提交 {ahead.length}</p>
              <CommitRows commits={ahead} />
            </div>
          )}
        </section>
        {props.history && props.history.items.length > 0 && (
          <section className="space-y-2">
            <h2 className="text-sm text-muted-foreground">最近发布</h2>
            <HistoryRows items={props.history.items} githubUrl={o.githubUrl} />
          </section>
        )}
      </div>
      {confirm && (
        <PublishDialog
          changes={confirm}
          onClose={() => setConfirm(null)}
          onStarted={(v) => {
            qc.setQueryData(OVERVIEW_KEY, v);
            setConfirm(null);
          }}
        />
      )}
    </div>
  );
}
