// 附件 UI: pictures as small squares (click → the viewer), other files as chips (click → their own app);
// while editing each has an ×. The paperclip button picks files; drops and pastes reach the target
// they land in (target.ts). A file not on this Mac shows dashed and greyed.
import { useEffect, useRef, useState, type ReactNode } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ChevronLeft, ChevronRight, ExternalLink, FileText, ImageOff, Paperclip, X } from "lucide-react";
import { errorText, useToast } from "@/shared/feedback";
import { Field } from "@/shared/ui";
import { attachmentsApi, fileSize, fileSrc, type FileInfo } from "./api";
import { useAttachTarget } from "./target";

const MISSING = "这个附件不在这台 Mac 上";

function Remove(props: { name: string; onClick: () => void }) {
  return (
    <button
      type="button"
      aria-label={`移除「${props.name}」`}
      title="移除"
      onClick={(e) => {
        e.stopPropagation();
        props.onClick();
      }}
      className="absolute -top-1.5 -right-1.5 hidden rounded-full border bg-card p-0.5 text-muted-foreground shadow-sm group-hover/att:block hover:text-foreground focus-visible:block"
    >
      <X size={11} />
    </button>
  );
}

/** The viewer: one picture at a time, ← → between them, Esc / a click outside closes. */
function Viewer(props: { images: FileInfo[]; index: number; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [i, setI] = useState(props.index);
  const toast = useToast();
  const n = props.images.length;
  const img = props.images[i];
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const step = (by: number) => setI((x) => (x + by + n) % n);
  return (
    <dialog
      ref={ref}
      aria-label={img.name}
      // React passes these on to a dialog the viewer sits in (a goal's): they stop here
      onCancel={(e) => {
        e.preventDefault();
        e.stopPropagation();
        props.onClose();
      }}
      onClick={(e) => {
        e.stopPropagation();
        if (e.target === e.currentTarget) props.onClose();
      }}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "ArrowLeft" && n > 1) step(-1);
        if (e.key === "ArrowRight" && n > 1) step(1);
      }}
      className="m-0 h-full max-h-none w-full max-w-none bg-transparent p-0 backdrop:bg-black/80"
    >
      <div className="flex h-full flex-col items-center justify-center gap-3 p-8" onClick={(e) => e.target === e.currentTarget && props.onClose()}>
        <img src={fileSrc(img.id)} alt={img.name} className="max-h-[calc(100%-3rem)] max-w-full rounded-md object-contain shadow-2xl" />
        <div className="flex items-center gap-3 text-sm text-white/85">
          {n > 1 && (
            <button type="button" aria-label="上一张" onClick={() => step(-1)} className="rounded-full p-1 hover:bg-white/15">
              <ChevronLeft size={18} />
            </button>
          )}
          <span className="max-w-[50vw] truncate">{img.name}</span>
          {n > 1 && (
            <span className="text-white/60 tabular-nums">
              {i + 1} / {n}
            </span>
          )}
          {n > 1 && (
            <button type="button" aria-label="下一张" onClick={() => step(1)} className="rounded-full p-1 hover:bg-white/15">
              <ChevronRight size={18} />
            </button>
          )}
          <button
            type="button"
            onClick={() => void attachmentsApi.open(img.id).catch((e) => toast(errorText(e), "error"))}
            className="flex items-center gap-1 rounded-control px-2 py-1 hover:bg-white/15"
          >
            <ExternalLink size={13} /> 用默认程序打开
          </button>
          <button type="button" aria-label="关闭" onClick={props.onClose} className="rounded-full p-1 hover:bg-white/15">
            <X size={16} />
          </button>
        </div>
      </div>
    </dialog>
  );
}

export function AttachmentStrip(props: { files: FileInfo[]; onRemove?: (id: number) => void; className?: string }) {
  const toast = useToast();
  const [viewing, setViewing] = useState<number | null>(null);
  const images = props.files.filter((f) => f.kind === "image" && !f.missing);
  const others = props.files.filter((f) => f.kind !== "image" || f.missing);
  if (props.files.length === 0) return null;
  return (
    <div className={`space-y-2 ${props.className ?? ""}`}>
      {images.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {images.map((f, i) => (
            <div key={f.id} className="group/att relative">
              <button
                type="button"
                title={f.name}
                aria-label={`看大图「${f.name}」`}
                onClick={() => setViewing(i)}
                className="block size-20 overflow-hidden rounded-md border bg-muted outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <img src={fileSrc(f.id)} alt="" loading="lazy" draggable={false} className="size-full object-cover" />
              </button>
              {props.onRemove && <Remove name={f.name} onClick={() => props.onRemove!(f.id)} />}
            </div>
          ))}
        </div>
      )}
      {others.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {others.map((f) => (
            <div key={f.id} className="group/att relative min-w-0">
              <button
                type="button"
                title={f.missing ? MISSING : f.name}
                disabled={f.missing}
                onClick={() => void attachmentsApi.open(f.id).catch((e) => toast(errorText(e), "error"))}
                className={`flex max-w-64 min-w-0 items-center gap-1.5 rounded-control border px-2 py-1 text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring ${f.missing ? "border-dashed text-muted-foreground" : ""}`}
              >
                {f.missing ? <ImageOff size={13} className="shrink-0" /> : <FileText size={13} className="shrink-0 text-muted-foreground" />}
                <span className="min-w-0 truncate">{f.name}</span>
                {!f.missing && <span className="shrink-0 text-xs text-muted-foreground">{fileSize(f.bytes)}</span>}
              </button>
              {props.onRemove && <Remove name={f.name} onClick={() => props.onRemove!(f.id)} />}
            </div>
          ))}
        </div>
      )}
      {viewing !== null && <Viewer images={images} index={Math.min(viewing, images.length - 1)} onClose={() => setViewing(null)} />}
    </div>
  );
}

/** Opens the file picker and stores what was chosen. */
export async function pickAttachments(): Promise<FileInfo[]> {
  const chosen = await openDialog({ multiple: true, title: "添加附件" });
  const paths = chosen === null ? [] : Array.isArray(chosen) ? chosen : [chosen];
  return paths.length ? attachmentsApi.addPaths(paths) : [];
}

export function AttachButton(props: { onFiles: (files: FileInfo[]) => void; children?: ReactNode }) {
  const toast = useToast();
  const pick = () => void pickAttachments().then(props.onFiles, (e) => toast(errorText(e), "error"));
  return props.children ? (
    <button
      type="button"
      onClick={pick}
      className="-my-0.5 inline-flex shrink-0 items-center gap-1 rounded-control px-1.5 py-0.5 text-xs text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
    >
      <Paperclip size={12} strokeWidth={1.75} /> {props.children}
    </button>
  ) : (
    <button
      type="button"
      aria-label="添加附件"
      title="添加附件"
      onClick={pick}
      className="inline-flex shrink-0 items-center justify-center rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
    >
      <Paperclip size={15} strokeWidth={1.75} />
    </button>
  );
}

/** Adds without repeats, keeping the order. */
export const withFiles = (have: FileInfo[], add: FileInfo[]) => [...have, ...add.filter((f) => !have.some((h) => h.id === f.id))];

/** 「附件」 in a side panel or dialog: its files take drops and pastes anywhere in it. */
export function AttachmentField(props: { files: FileInfo[]; onChange: (files: FileInfo[]) => void }) {
  const toast = useToast();
  const anchor = useRef<HTMLDivElement>(null);
  const area = useRef<HTMLElement | null>(null);
  useEffect(() => {
    area.current = anchor.current?.closest("aside, dialog") ?? anchor.current;
  }, []);
  const latest = useRef(props.files);
  latest.current = props.files;
  const dropping = useAttachTarget(area, (fs) => props.onChange(withFiles(latest.current, fs)), (e) => toast(errorText(e), "error"));
  return (
    <div ref={anchor} className={`-m-1 rounded-card p-1 ${dropping ? "ring-2 ring-primary" : ""}`}>
      <Field label="附件" action={<AttachButton onFiles={(fs) => props.onChange(withFiles(latest.current, fs))}>添加</AttachButton>}>
        <AttachmentStrip files={props.files} onRemove={(id) => props.onChange(props.files.filter((f) => f.id !== id))} />
      </Field>
    </div>
  );
}
