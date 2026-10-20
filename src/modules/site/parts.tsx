// Pieces the 呦呦漫游 editors share: a document's load / save / ⌘S with its unsaved state (useDoc), the
// bar above it (中文 · English · 并排, 预览, 保存, ⋯), a Markdown body with 小拾's 润色 and 按中文起草
// (BodyField, useEnglishDraft), row controls (上移、下移、移出) and a list of pictures (ImageList).
import { useEffect, useRef, useState, type ComponentProps, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ArrowDown, ArrowUp, ImagePlus, MoreHorizontal, X } from "lucide-react";
import { AssistAction, PolishBox, useAssistOn } from "@/modules/picky/Assist";
import { Dropdown, type DropdownOption } from "@/shared/Dropdown";
import { useDropTarget } from "@/shared/attachments/target";
import { Button, IconButton, Segmented, fieldClass } from "@/shared/ui";
import { MdEditor, type MdEditorHandle } from "./MdEditor";
import { siteAssist, siteSrc, workApi, type Draft, type Lang } from "./api";

export type View = Lang | "both";
export const LABEL: Record<Lang, string> = { zh: "中文", en: "English" };
export const IMAGE_EXT = ["jpg", "jpeg", "png", "webp", "gif", "svg", "avif", "heic", "heif", "tif", "tiff"];
export const GHOST = "inline-flex items-center gap-1 rounded-control px-2 py-1.5 text-sm text-muted-foreground outline-none transition-colors duration-150 hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring";

/** The editor shows a body without the file's last newline (Rust writes it back). */
export const shown = (body: string) => body.replace(/\n$/, "");
export const stored = (text: string) => (text.trim() ? `${text}\n` : "");

export async function pickFiles(multiple: boolean, images = true): Promise<string[]> {
  const p = await openDialog({ multiple, title: images ? "选择图片" : "选择文件", filters: images ? [{ name: "图片", extensions: IMAGE_EXT }] : undefined });
  return p === null ? [] : Array.isArray(p) ? p : [p];
}

/** Files into the site (`target`: wander:<slug> | now | about | career | portraits | artworks | cv:zh | cv:en |
 * asset:image | asset:document). */
export const addTo = (target: string) => (take: { paths?: string[]; files?: File[] }) =>
  take.paths ? workApi.addFiles(target, take.paths) : Promise.all((take.files ?? []).map((f) => workApi.addFileTo(target, f)));

// ---------- load / save ----------

/** A document loaded from Rust and edited here: saved with 保存 / ⌘S; `dirty` goes up to the page guard. */
export function useDoc<T>(key: unknown[], load: () => Promise<T>, saveFn: (t: T) => Promise<T>, setDirty: (d: boolean) => void, onError: (e: unknown) => void, onSaved?: () => void) {
  const { data: loaded, refetch } = useQuery({ queryKey: key, queryFn: load, gcTime: 0, staleTime: 0 });
  const [saved, setSaved] = useState<T | null>(null);
  const [draft, setDraft] = useState<T | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (loaded) {
      setSaved(loaded);
      setDraft(loaded);
    }
  }, [loaded]);
  const dirty = !!draft && !!saved && JSON.stringify(draft) !== JSON.stringify(saved);
  useEffect(() => setDirty(dirty), [dirty]); // eslint-disable-line react-hooks/exhaustive-deps
  const latest = useRef(draft);
  latest.current = draft;
  /** The document as now on disk (also when nothing changed); null when saving failed. */
  const save = async (): Promise<T | null> => {
    const sent = latest.current;
    if (!sent || !saved) return null;
    if (JSON.stringify(sent) === JSON.stringify(saved)) return saved;
    setSaving(true);
    try {
      const t = await saveFn(sent);
      setSaved(t);
      setDraft((d) => (d === sent ? t : d));
      onSaved?.();
      return t;
    } catch (e) {
      onError(e);
      return null;
    } finally {
      setSaving(false);
    }
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && !e.shiftKey && e.key.toLowerCase() === "s" && !document.querySelector("dialog[open]")) {
        e.preventDefault();
        void save();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
  const reload = async () => {
    const r = await refetch();
    if (r.data) {
      setSaved(r.data);
      setDraft(r.data);
    }
  };
  /** Replaces both (after create / rename…). */
  const reset = (t: T) => {
    setSaved(t);
    setDraft(t);
  };
  return { draft, setDraft, saved, dirty, saving, save, reload, reset };
}

/** The bar above an editor: the language view on the left; 预览 · 保存 · ⋯ on the right. */
export function DocBar(props: { view?: View; setView?: (v: View) => void; left?: ReactNode; extra?: ReactNode; onPreview?: () => void; previewing?: boolean; onSave: () => void; dirty: boolean; saving: boolean; menu?: DropdownOption<string>[]; onMenu?: (v: string) => void }) {
  return (
    <div className="flex shrink-0 items-center justify-between gap-3 border-b px-6 py-2">
      <div className="flex min-w-0 items-center gap-3">
        {props.view && props.setView && <Segmented ariaLabel="语言" value={props.view} onChange={props.setView} options={[{ value: "zh", label: "中文" }, { value: "en", label: "English" }, { value: "both", label: "并排" }]} />}
        {props.left}
      </div>
      <div className="flex items-center gap-1.5">
        {props.extra}
        {props.onPreview && (
          <Button disabled={props.previewing || props.saving} onClick={props.onPreview}>
            {props.previewing ? "正在打开…" : "预览"}
          </Button>
        )}
        <Button variant="primary" disabled={!props.dirty || props.saving} onClick={props.onSave}>
          {props.saving ? "正在保存…" : "保存"}
        </Button>
        {props.menu && props.onMenu && (
          <Dropdown ariaLabel="更多" plain align="end" value={null as string | null} onChange={(v) => v && props.onMenu!(v)} options={props.menu} renderTrigger={() => <MoreHorizontal size={16} strokeWidth={1.5} />} triggerClassName={GHOST} />
        )}
      </div>
    </div>
  );
}

/** Opens a page of the local site in Pomelo's preview window (busy while the dev server starts). */
export function usePreview(onError: (e: unknown) => void) {
  const [previewing, setPreviewing] = useState(false);
  const preview = async (path: string, before?: () => Promise<unknown>) => {
    if (before && !(await before())) return;
    setPreviewing(true);
    try {
      await workApi.preview(path);
    } catch (e) {
      onError(e);
    } finally {
      setPreviewing(false);
    }
  };
  return { previewing, preview };
}

// ---------- 按中文起草 ----------

/** 按中文起草 for `key` (a work's slug, now:2026-07, about…): Rust writes the English in memory; `apply`
 * takes what is written so far, then it is cleared. */
export function useEnglishDraft(key: string, apply: (d: Draft) => void, onError: (e: unknown) => void, ready: boolean) {
  const { data: job, refetch } = useQuery({
    queryKey: ["picky", "site-draft"],
    queryFn: siteAssist.draft,
    gcTime: 0,
    staleTime: 0,
    refetchIntervalInBackground: true,
    refetchInterval: (q) => (q.state.data?.running ? 400 : false),
  });
  const mine = job?.slug === key ? job : null;
  useEffect(() => {
    if (!mine || !ready) return;
    apply(mine);
    if (!mine.running) {
      if (mine.error) onError(mine.error);
      void siteAssist.draftClear().then(() => refetch());
    }
  }, [mine?.title, mine?.summary, mine?.body, mine?.running, mine?.error, ready]); // eslint-disable-line react-hooks/exhaustive-deps
  const start = async (zh: { title: string; summary: string; body: string }) => {
    try {
      await siteAssist.draftRun(key, zh.title, zh.summary, zh.body);
      void refetch();
    } catch (e) {
      onError(e);
    }
  };
  const stop = () => void siteAssist.draftStop().then(() => refetch(), onError);
  return { job, mine, drafting: !!mine?.running, start, stop };
}

/** 按中文起草: the action, then what it is doing and 停止. */
export function DraftAction(props: { job: Draft | undefined; mine: Draft | null; onStart: () => void; onStop: () => void }) {
  if (props.mine?.running)
    return (
      <span className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
        <span className="min-w-0 animate-pulse truncate">{props.mine.status ?? "正在起草…"}</span>
        <button type="button" onClick={props.onStop} className="shrink-0 rounded px-1 hover:text-foreground">
          停止
        </button>
      </span>
    );
  return (
    <AssistAction disabled={!!props.job?.running} onClick={props.onStart}>
      按中文起草
    </AssistAction>
  );
}

// ---------- the body ----------

/** 润色 on a body: the selection when there is one, else the whole text. */
function BodyPolish(props: { editor: MdEditorHandle | null; body: string; setBody: (text: string) => void; onClose: () => void }) {
  const [range] = useState(() => {
    const [s, e] = props.editor?.selection() ?? [0, 0];
    return s < e ? ([s, e] as const) : null;
  });
  const text = shown(props.body);
  return (
    <PolishBox
      module="site"
      value={range ? text.slice(range[0], range[1]) : text}
      onReplace={(t) => {
        if (range && props.editor) props.editor.replaceRange(range[0], range[1], t);
        else props.setBody(stored(t));
        props.onClose();
      }}
      onClose={props.onClose}
    />
  );
}

/** A Markdown body (stored form in and out) with 润色 (and `draft`: 按中文起草) above it. */
export function BodyField(props: { lang: Lang; label: string; body: string; setBody: (b: string) => void; editor?: (el: MdEditorHandle | null) => void; addImages: (take: { paths?: string[]; files?: File[] }) => Promise<string[]>; onError: (e: unknown) => void; draft?: ReactNode; locked?: boolean; note?: string | null }) {
  const assistOn = useAssistOn();
  const [polishing, setPolishing] = useState(false);
  const handle = useRef<MdEditorHandle | null>(null);
  return (
    <div>
      {assistOn && (
        <div className="flex min-h-6 items-center justify-end gap-1">
          {props.note && <span className="mr-auto truncate text-xs text-muted-foreground">{props.note}</span>}
          {props.draft}
          {!props.locked && shown(props.body).trim() && (
            <AssistAction disabled={polishing} onClick={() => setPolishing(true)}>
              润色
            </AssistAction>
          )}
        </div>
      )}
      {polishing && (
        <div className="mb-2">
          <BodyPolish editor={handle.current} body={props.body} setBody={props.setBody} onClose={() => setPolishing(false)} />
        </div>
      )}
      <div className="pt-1">
        <MdEditor
          ref={(el) => {
            handle.current = el;
            props.editor?.(el);
          }}
          label={`${props.label}（${LABEL[props.lang]}）`}
          value={shown(props.body)}
          readOnly={props.locked}
          onChange={(t) => props.setBody(stored(t))}
          onImages={props.addImages}
          onError={props.onError}
        />
      </div>
    </div>
  );
}

/** A text box of its own width (TextInput is always full width). */
export function SizedInput({ className, ...rest }: ComponentProps<"input">) {
  return <input type="text" autoComplete="off" spellCheck={false} className={`${fieldClass} ${className ?? ""}`} {...rest} />;
}

// ---------- rows ----------

/** 上移 · 下移 · 移出 for row `i` of `n`. */
export function RowControls(props: { i: number; n: number; onMove: (to: number) => void; onRemove: () => void; removeLabel?: string; vertical?: boolean }) {
  return (
    <div className={`flex shrink-0 ${props.vertical ? "flex-col" : ""}`}>
      <IconButton label="上移" disabled={props.i === 0} onClick={() => props.onMove(props.i - 1)}>
        <ArrowUp size={14} />
      </IconButton>
      <IconButton label="下移" disabled={props.i === props.n - 1} onClick={() => props.onMove(props.i + 1)}>
        <ArrowDown size={14} />
      </IconButton>
      <IconButton label={props.removeLabel ?? "移出"} danger onClick={props.onRemove}>
        <X size={14} />
      </IconButton>
    </div>
  );
}

export function moved<T>(list: T[], from: number, to: number): T[] {
  const out = [...list];
  const [x] = out.splice(from, 1);
  out.splice(to, 0, x);
  return out;
}

export const replaced = <T,>(list: T[], i: number, patch: Partial<T>): T[] => list.map((x, j) => (j === i ? { ...x, ...patch } : x));

/** A small "+ …" under a list. */
export function AddRow(props: { onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" onClick={props.onClick} className="rounded-control px-1 text-sm text-primary outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
      + {props.children}
    </button>
  );
}

// ---------- pictures ----------

/** Pictures (stored paths) in order: add (选择… / 资源库… / dropped), move, remove. */
export function ImageList(props: { items: string[]; onChange: (items: string[]) => void; target: string; onError: (e: unknown) => void; onPickAsset: () => void }) {
  const box = useRef<HTMLDivElement>(null);
  const add = (p: Promise<string[]>) => void p.then((urls) => props.onChange([...props.items, ...urls.map((u) => `public${u}`)]), props.onError);
  const take = addTo(props.target);
  const dropping = useDropTarget(box, { paths: (paths) => add(take({ paths })), files: (files) => add(take({ files })) });
  return (
    <div ref={box} className={`space-y-2 rounded-control ${dropping ? "ring-2 ring-primary" : ""}`}>
      <div className="grid grid-cols-2 gap-2">
        {props.items.map((src, i) => (
          <div key={`${src}-${i}`} className="group/img relative">
            <img src={siteSrc(src)} alt="" className="aspect-square w-full rounded-control border bg-muted object-cover" />
            <div className="absolute top-1 right-1 rounded-control bg-card/90 opacity-0 transition-opacity duration-150 group-focus-within/img:opacity-100 group-hover/img:opacity-100">
              <RowControls i={i} n={props.items.length} onMove={(to) => props.onChange(moved(props.items, i, to))} onRemove={() => props.onChange(props.items.filter((_, j) => j !== i))} vertical />
            </div>
          </div>
        ))}
      </div>
      <div className="flex gap-x-1 text-xs">
        <button type="button" className="inline-flex items-center gap-1 rounded px-1 text-muted-foreground hover:text-foreground" onClick={() => void pickFiles(true).then((paths) => paths.length && add(take({ paths })))}>
          <ImagePlus size={12} strokeWidth={1.5} /> 选择…
        </button>
        <button type="button" className="rounded px-1 text-muted-foreground hover:text-foreground" onClick={props.onPickAsset}>
          资源库…
        </button>
      </div>
    </div>
  );
}
