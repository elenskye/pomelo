// 见字如面's moving in and out: the export dialog (每次弹设置: format, PDF / Word's style and what's printed on, where it
// goes; remembered), 从 Bear 搬家 (what Bear has first, then once), importing Markdown / TextBundle files and folders,
// and what came over (or didn't). The rules are Rust's (articles/export.rs, import.rs, print.rs).
import { useEffect, useState } from "react";
import { open as pickPath } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Modal } from "@/shared/Modal";
import { errorText, useToast } from "@/shared/feedback";
import { Check } from "@/shared/icons";
import { useSetting } from "@/shared/settings";
import { Button, Segmented } from "@/shared/ui";
import { ARTICLES_KEY, articlesApi, type ExportFormat, type ExportOptions, type ExportReport, type ExportStyle, type ExportTarget, type ImportReport, type Left } from "./api";

type Saved = Omit<ExportOptions, "dest"> & { dest: string | null };
const DEFAULTS: Saved = { format: "pdf", style: "pomelo", toc: true, header: true, meta: false, dest: null };
const isSaved = (v: unknown): v is Saved => {
  const s = v as Saved;
  return !!s && ["md", "pdf", "docx"].includes(s.format) && ["pomelo", "print"].includes(s.style) && typeof s.toc === "boolean" && typeof s.header === "boolean" && typeof s.meta === "boolean";
};

/** /Users/x/Downloads/见字如面 → 下载 / 见字如面 */
function shown(path: string) {
  const m = /^\/Users\/[^/]+\/?(.*)$/.exec(path);
  if (!m) return path.split("/").filter(Boolean).join(" / ");
  const parts = m[1].split("/").filter(Boolean);
  const names: Record<string, string> = { Downloads: "下载", Documents: "文稿", Desktop: "桌面" };
  if (parts.length && names[parts[0]]) parts[0] = names[parts[0]];
  return parts.length ? parts.join(" / ") : "个人文件夹";
}

function MiniPages(props: { kind: ExportStyle }) {
  const lines = (n: number) => Array.from({ length: n }, (_, i) => <i key={i} className="ar-mini-l" style={{ width: `${80 - (i % 3) * 12}%` }} />);
  return (
    <div className={`ar-mini ar-mini-${props.kind}`} data-theme="light" aria-hidden="true">
      <div className="ar-mp">
        <i className="ar-mini-t" />
        {props.kind === "pomelo" && <i className="ar-mini-m" />}
        {lines(4)}
        <i className="ar-mini-gap" />
        {lines(5)}
      </div>
      <div className="ar-mp">
        {lines(6)}
        <i className="ar-mini-block" />
        {lines(5)}
      </div>
    </div>
  );
}

function Check2(props: { label: string; on: boolean; seed: number; onChange: (v: boolean) => void }) {
  return (
    <button type="button" role="checkbox" aria-checked={props.on} className="pm-check ar-check" onClick={() => props.onChange(!props.on)}>
      <span data-hand="check" data-seed={props.seed} />
      <span>{props.label}</span>
    </button>
  );
}

function Row(props: { label: string; children: React.ReactNode }) {
  return (
    <div className="ar-drow">
      <span className="ar-dlab">{props.label}</span>
      <div className="ar-dval">{props.children}</div>
    </div>
  );
}

/** What didn't come over (or came over differently), as a short list. */
function LeftList(props: { left: Left[]; fixes?: string[] }) {
  if (!props.left.length && !props.fixes?.length) return null;
  return (
    <div className="ar-left pm-scroll -m-1 max-h-[240px] overflow-y-auto p-1">
      {props.left.map((l, i) => (
        <p key={`l${i}`} className="text-sm">
          <span className="font-bold">{l.title}</span>
          <span className="text-muted-foreground">：{l.what}</span>
        </p>
      ))}
      {props.fixes?.map((f, i) => (
        <p key={`f${i}`} className="text-sm text-muted-foreground">
          {f}
        </p>
      ))}
    </div>
  );
}

/** 导出：每次弹设置. `label`: 「东京的第三天」· 1 篇 / #读书 · 12 篇 / 全部 · 143 篇. */
export function ExportDialog(props: { target: ExportTarget; label: string; onClose: () => void }) {
  const toast = useToast();
  const [saved, setSaved] = useSetting<Saved>("articles.export", DEFAULTS, isSaved);
  const home = useQuery({ queryKey: [...ARTICLES_KEY, "export-dir"], queryFn: articlesApi.exportDir, staleTime: Infinity });
  const o = { ...saved, dest: saved.dest ?? home.data ?? "" };
  const set = (p: Partial<Saved>) => setSaved({ ...saved, ...p });
  const [busy, setBusy] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<ExportReport | null>(null);

  useEffect(() => {
    if (!busy) return;
    const off = listen<{ done: number; total: number }>("articles-export", (e) => setBusy(e.payload));
    return () => void off.then((f) => f());
  }, [!!busy]); // eslint-disable-line react-hooks/exhaustive-deps

  const choose = async () => {
    const p = await pickPath({ directory: true, title: "导出到", defaultPath: o.dest || undefined }).catch(() => null);
    if (typeof p === "string") set({ dest: p });
  };
  const run = async () => {
    if (busy) return;
    setError(null);
    setBusy({ done: 0, total: props.target.ids?.length ?? 0 });
    try {
      const r = await articlesApi.export({ target: props.target, options: o });
      if (!r.left.length) {
        toast(r.written === 1 ? "导出好了" : `导出了 ${r.written} 篇`);
        void articlesApi.reveal(r.file ?? r.folder).catch(() => {});
        props.onClose();
      } else setDone(r);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  if (done)
    return (
      <Modal
        title={done.written ? `导出了 ${done.written} 篇` : "没有导出"}
        wide
        onClose={props.onClose}
        cancelLabel="好"
        error={null}
        footerLeft={done.written > 0 && <Button onClick={() => void articlesApi.reveal(done.file ?? done.folder).catch((e) => toast(errorText(e), "error"))}>在访达里显示</Button>}
      >
        <LeftList left={done.left} />
      </Modal>
    );

  const printed = o.format !== "md";
  return (
    <Modal
      title="导出"
      wide
      onClose={() => !busy && props.onClose()}
      onSubmit={(e) => {
        e.preventDefault();
        void run();
      }}
      submitLabel={busy ? (busy.total > 1 ? `正在导出 ${busy.done} / ${busy.total}` : "正在导出…") : "导出"}
      submitDisabled={!!busy || !o.dest}
      error={error}
    >
      <p className="-mt-3 text-sm text-muted-foreground">{props.label}</p>
      <Row label="格式">
        <Segmented<ExportFormat> ariaLabel="格式" value={o.format} onChange={(format) => set({ format })} options={[{ value: "md", label: "Markdown" }, { value: "pdf", label: "PDF" }, { value: "docx", label: "Word" }]} />
      </Row>
      {printed && (
        <Row label="样式">
          <div className="ar-styles">
            {(["pomelo", "print"] as const).map((k, i) => (
              <button key={k} type="button" aria-pressed={o.style === k} className={`ar-sty ${o.style === k ? "on" : ""}`} data-hand="cell" data-fill="card" data-seed={1480 + i} onClick={() => set({ style: k })}>
                {o.style === k && <span className="ar-sty-pk" data-hand="pick" data-seed={1484 + i} />}
                <MiniPages kind={k} />
                <span className="ar-stname">
                  {k === "pomelo" ? "柚儿园" : "印刷"}
                  <Check size={16} />
                </span>
              </button>
            ))}
          </div>
        </Row>
      )}
      {printed && (
        <Row label="印上">
          <div className="ar-checks">
            <Check2 label="目录" seed={1490} on={o.toc} onChange={(toc) => set({ toc })} />
            <Check2 label="页眉和页码" seed={1491} on={o.header} onChange={(header) => set({ header })} />
            <Check2 label="标签和日期" seed={1492} on={o.meta} onChange={(meta) => set({ meta })} />
          </div>
        </Row>
      )}
      <Row label="存到">
        <div className="ar-dpath">
          <span className="pm-field ar-dfield" data-hand="field" data-seed={1494} title={o.dest}>
            {o.dest ? shown(o.dest) : "…"}
          </span>
          <Button onClick={() => void choose()}>选择…</Button>
        </div>
      </Row>
    </Modal>
  );
}

/** What came over: counts, then what to look at. */
function ImportDone(props: { title: string; r: ImportReport; onClose: () => void }) {
  const r = props.r;
  return (
    <Modal title={props.title} wide onClose={props.onClose} cancelLabel="好" error={null}>
      <p className="text-sm">
        {r.imported} 篇 · {r.files} 个附件 · {r.tags} 个标签
      </p>
      <LeftList left={r.left} fixes={r.fixes} />
    </Modal>
  );
}

/** 从 Bear 搬家: what Bear has, then once (Bear itself isn't touched). */
export function BearDialog(props: { onClose: () => void }) {
  const qc = useQueryClient();
  const preview = useQuery({ queryKey: [...ARTICLES_KEY, "bear"], queryFn: articlesApi.bearPreview, retry: false, gcTime: 0 });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<ImportReport | null>(null);
  if (done) return <ImportDone title="从 Bear 搬过来了" r={done} onClose={props.onClose} />;
  const p = preview.data;
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      setDone(await articlesApi.bearImport());
      void qc.invalidateQueries({ queryKey: ARTICLES_KEY });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const once = !!p && p.here * 2 > p.notes + p.archived + p.trashed - p.locked;
  return (
    <Modal
      title="从 Bear 搬家"
      wide
      onClose={() => !busy && props.onClose()}
      onSubmit={(e) => {
        e.preventDefault();
        void run();
      }}
      submitLabel={busy ? "正在搬…" : "搬过来"}
      submitDisabled={busy || !p || once}
      error={error ?? (preview.error ? errorText(preview.error) : null)}
    >
      {!p && !preview.error && <p className="text-sm text-muted-foreground">正在看 Bear 里有什么…</p>}
      {p && (
        <div className="space-y-1 text-sm">
          <p>
            Bear 里有 {p.notes} 篇{p.archived ? `、归档 ${p.archived} 篇` : ""}
            {p.trashed ? `、废纸篓 ${p.trashed} 篇` : ""}，{p.tags} 个标签，{p.files} 个附件。
          </p>
          {p.locked > 0 && <p className="text-muted-foreground">{p.locked} 篇加密了，Bear 的命令行读不出来，搬不过来。</p>}
          {once ? <p className="text-destructive">已经搬过了（{p.here} 篇在这里）。</p> : <p className="text-muted-foreground">Bear 那边什么都不动。</p>}
        </div>
      )}
    </Modal>
  );
}

/** 导入 Markdown / TextBundle: files, or a folder (its sub-folders become tags). */
export function useImportFiles() {
  const qc = useQueryClient();
  const toast = useToast();
  const [done, setDone] = useState<ImportReport | null>(null);
  const run = async (folder: boolean) => {
    const picked = await pickPath(folder ? { directory: true, title: "导入文件夹" } : { multiple: true, title: "导入 Markdown", filters: [{ name: "Markdown、TextBundle", extensions: ["md", "markdown", "txt", "textbundle", "textpack"] }] }).catch(() => null);
    const paths = picked === null ? [] : Array.isArray(picked) ? picked : [picked];
    if (!paths.length) return;
    try {
      const r = await articlesApi.importFiles(paths);
      void qc.invalidateQueries({ queryKey: ARTICLES_KEY });
      if (r.left.length || r.fixes.length) setDone(r);
      else toast(`导入了 ${r.imported} 篇`);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const dialog = done ? <ImportDone title="导入好了" r={done} onClose={() => setDone(null)} /> : null;
  return [run, dialog] as const;
}
