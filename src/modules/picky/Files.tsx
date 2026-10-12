// Attachments and project files on the page: chips in the input, cards on questions, the attachment
// library, a project's files (versions, index) and the input's 资料 menu. Rules live in Rust
// (files.rs / index.rs); copies are opened in their default app.
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Check, CircleCheck, Ellipsis, Library, Plus, TriangleAlert, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button, IconButton } from "@/shared/ui";
import {
  FILE_EXTENSIONS,
  LIBRARY_FILES_KEY,
  currentFile,
  fileDetail,
  pickyApi,
  projectFilesKey,
  usableFile,
  useProjectFiles,
  type FileInfo,
  type MessageFile,
  type ProjectFile,
} from "./api";
import { toolClass } from "./Composer";

/** PDF / MD / TXT */
export function FileBadge(props: { format: FileInfo["format"] }) {
  const t = props.format === "PDF" ? "PDF" : props.format === "Markdown" ? "MD" : "TXT";
  return (
    <span aria-hidden className={`inline-flex h-5 w-8 shrink-0 items-center justify-center rounded text-[10px] font-bold ${t === "PDF" ? "bg-primary/15 text-primary" : "bg-muted text-muted-foreground"}`}>
      {t}
    </span>
  );
}

/** A file in the input (an attachment of the draft, or a project file chosen for it). */
export function FileChip(props: { file: FileInfo; project?: boolean; onOpen: () => void; onRemove: () => void }) {
  return (
    <span className={`inline-flex max-w-64 items-center gap-1.5 rounded-control py-1 pr-1 pl-1.5 text-xs ${props.project ? "bg-primary/10" : "bg-muted"}`}>
      <button type="button" onClick={props.onOpen} className="flex min-w-0 items-center gap-1.5 outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <FileBadge format={props.file.format} />
        <span className="truncate">{props.file.name}</span>
        <span className="shrink-0 text-muted-foreground">{fileDetail(props.file)}</span>
      </button>
      <button type="button" aria-label={`移除 ${props.file.name}`} onClick={props.onRemove} className="rounded p-0.5 text-muted-foreground hover:bg-background/60 hover:text-foreground">
        <X size={12} />
      </button>
    </span>
  );
}

/** Cards above a question: what it was sent with. */
export function MessageFiles(props: { files: MessageFile[]; onOpen: (id: number) => void }) {
  if (!props.files.length) return null;
  return (
    <div data-find-skip className="mb-1.5 flex max-w-[85%] flex-wrap justify-end gap-1.5">
      {props.files.map((f, i) => (
        <button
          key={`${f.id}-${i}`}
          type="button"
          disabled={f.deleted}
          onClick={() => props.onOpen(f.id)}
          className="flex max-w-72 items-center gap-2 rounded-card border bg-card px-2.5 py-1.5 text-left outline-none hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60"
        >
          <FileBadge format={f.format} />
          <span className="flex min-w-0 flex-col">
            <span className="truncate text-sm">{f.name}</span>
            <span className="truncate text-xs text-muted-foreground">{[f.label, fileDetail(f)].filter(Boolean).join(" · ")}</span>
          </span>
        </button>
      ))}
    </div>
  );
}

/** Pick files (txt / md / pdf) and store a copy of each; errors are shown one by one. */
export function useImport() {
  const toast = useToast();
  const qc = useQueryClient();
  const importPaths = async (paths: string[]) => {
    const out: FileInfo[] = [];
    for (const p of paths) {
      try {
        out.push(await pickyApi.importFile(p));
      } catch (e) {
        toast(`没能添加「${p.split("/").pop()}」：${errorText(e)}`, "error");
      }
    }
    void qc.invalidateQueries({ queryKey: LIBRARY_FILES_KEY });
    return out;
  };
  const choose = async (multiple: boolean) => {
    const picked = await openDialog({ multiple, directory: false, filters: [{ name: "文本、Markdown、PDF", extensions: FILE_EXTENSIONS }] });
    const paths = picked == null ? [] : Array.isArray(picked) ? picked : [picked];
    return paths.length ? importPaths(paths) : [];
  };
  return { choose, importPaths };
}

export function openFile(id: number, toast: ReturnType<typeof useToast>) {
  void pickyApi.openFile(id).catch((e) => toast(errorText(e), "error"));
}

/** A modal panel with a title row and a close button (not a form). */
export function Panel(props: { title: ReactNode; right?: ReactNode; onClose: () => void; children: ReactNode; width?: string }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  return (
    <dialog
      ref={ref}
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className={`m-auto flex max-h-[78vh] ${props.width ?? "w-[620px]"} max-w-[92vw] flex-col rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30`}
    >
      <div className="flex items-center gap-3 border-b px-5 py-3">
        <p className="min-w-0 truncate text-base font-bold">{props.title}</p>
        <div className="flex-1" />
        {props.right}
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      {props.children}
    </dialog>
  );
}

/** 附件库: stored copies (newest first); `pick` adds one (to the draft, or to a project). */
export function AttachmentLibrary(props: { pickLabel: string; disabled?: (f: FileInfo) => boolean; onPick: (f: FileInfo) => void; onClose: () => void }) {
  const toast = useToast();
  const { choose } = useImport();
  const list = useQuery({ queryKey: LIBRARY_FILES_KEY, queryFn: pickyApi.attachmentLibrary }).data ?? [];
  return (
    <Panel
      title="附件库"
      onClose={props.onClose}
      right={
        <Button onClick={() => void choose(true).then((fs) => fs.forEach(props.onPick))}>
          <Plus size={14} /> 添加文件…
        </Button>
      }
    >
      <div className="-m-1 min-h-40 overflow-y-auto p-1">
        <div className="px-3 py-2">
          {!list.length && <p className="px-2 py-6 text-center text-sm text-muted-foreground">暂无附件</p>}
          {list.map((f) => (
            <div key={f.id} className="flex items-center gap-3 rounded-control px-2 py-2 hover:bg-muted/50">
              <FileBadge format={f.format} />
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm">{f.name}</p>
                <p className="text-xs text-muted-foreground">{fileDetail(f)}</p>
              </div>
              <Button className="py-1" onClick={() => openFile(f.id, toast)}>
                打开
              </Button>
              <Button className="py-1" disabled={props.disabled?.(f)} onClick={() => props.onPick(f)}>
                {props.pickLabel}
              </Button>
            </div>
          ))}
        </div>
      </div>
    </Panel>
  );
}

function IndexControl(props: { file: ProjectFile; act: (p: Promise<unknown>) => void }) {
  const f = props.file;
  const s = f.index;
  const small = "rounded-control px-2 py-1 text-xs text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40";
  switch (s.kind) {
    case "none":
      return (
        <button type="button" className={small} disabled={!usableFile(f)} onClick={() => props.act(pickyApi.projectFileIndex(f.id, true))}>
          建立索引
        </button>
      );
    case "queued":
    case "building":
      return (
        <span className="flex items-center gap-2 text-xs text-muted-foreground">
          {s.kind === "building" && s.total > 0 ? (
            <span className="h-1.5 w-16 overflow-hidden rounded-full bg-muted">
              <span className="block h-full bg-primary" style={{ width: `${(s.done / s.total) * 100}%` }} />
            </span>
          ) : null}
          {s.kind === "queued" ? "等待建立索引" : s.total > 0 ? `正在建立索引 ${s.done}/${s.total}` : "正在建立索引…"}
          <button type="button" className={small} onClick={() => props.act(pickyApi.indexStop(f.id))}>
            停止
          </button>
        </span>
      );
    case "ready":
      return (
        <span className="flex items-center gap-1 text-xs text-muted-foreground">
          <CircleCheck size={13} /> 已建立索引
        </span>
      );
    case "failed":
      return (
        <span className="flex items-center gap-1 text-xs text-destructive" title={s.message}>
          <TriangleAlert size={13} /> 索引失败
          <button type="button" className={small} disabled={!usableFile(f)} onClick={() => props.act(pickyApi.indexRebuild(f.id))}>
            重试
          </button>
        </span>
      );
    case "incomplete":
      return (
        <button type="button" className={small} disabled={!usableFile(f)} onClick={() => props.act(pickyApi.indexRebuild(f.id))}>
          重新建立索引
        </button>
      );
  }
}

/** A project's files: add (a new file or from the library), open (old versions too), replace with a
 *  new version, index, remove. Each change can be undone. */
export function ProjectFiles(props: { projectId: number; projectName: string; onClose: () => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const { choose } = useImport();
  const [library, setLibrary] = useState(false);
  const files = useProjectFiles(props.projectId).data ?? [];
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: projectFilesKey(props.projectId) });
    void qc.invalidateQueries({ queryKey: LIBRARY_FILES_KEY });
    void qc.invalidateQueries({ queryKey: ["picky", "library"] });
  };
  const act = (p: Promise<unknown>) => void p.then(refresh, (e) => toast(errorText(e), "error"));
  const add = async (artifacts: FileInfo[]) => {
    for (const a of artifacts) await pickyApi.projectFileAdd(props.projectId, a.id).catch((e) => toast(errorText(e), "error"));
    refresh();
  };
  const replace = async (f: ProjectFile) => {
    const [next] = await choose(false);
    if (next) act(pickyApi.projectFileReplace(f.id, next.id));
  };
  const remove = async (f: ProjectFile) => {
    const name = currentFile(f)?.name ?? "资料";
    if (await confirm({ title: `移除资料「${name}」？`, message: "项目里的对话不再用它，索引一起删掉；用过它的消息仍会显示它。文件副本留在附件库。", confirmLabel: "移除", danger: true }))
      act(pickyApi.projectFileRemove(f.id));
  };
  return (
    <Panel
      title={`资料 · ${props.projectName}`}
      onClose={props.onClose}
      width="w-[680px]"
      right={
        <Dropdown<string | null>
          plain
          ariaLabel="添加资料"
          value={null}
          align="end"
          options={[
            { value: "file", label: "从文件选择…" },
            { value: "library", label: "从附件库…" },
          ]}
          onChange={(v) => (v === "file" ? void choose(true).then(add) : v === "library" ? setLibrary(true) : null)}
          renderTrigger={() => (
            <span className="flex items-center gap-1">
              <Plus size={14} /> 添加
            </span>
          )}
        />
      }
    >
      <div className="-m-1 min-h-40 overflow-y-auto p-1">
        <div className="px-3 py-2">
          {!files.length && <p className="px-2 py-6 text-center text-sm text-muted-foreground">还没有资料</p>}
          {files.map((f) => {
            const cur = currentFile(f);
            const old = f.versions.filter((v) => v.version !== f.currentVersion).reverse();
            return (
              <div key={f.id} className="flex items-center gap-3 rounded-control px-2 py-2 hover:bg-muted/50">
                {cur && <FileBadge format={cur.format} />}
                <div className="min-w-0 flex-1">
                  <button type="button" disabled={!usableFile(f)} onClick={() => cur && openFile(cur.id, toast)} className="block max-w-full truncate text-left text-sm hover:underline disabled:no-underline">
                    {cur?.name ?? "资料"}
                  </button>
                  <p className={`text-xs ${usableFile(f) ? "text-muted-foreground" : "text-destructive"}`}>
                    {[f.currentVersion > 1 ? `第 ${f.currentVersion} 版` : null, cur ? fileDetail(cur) : "文件已删除"].filter(Boolean).join(" · ")}
                  </p>
                </div>
                <IndexControl file={f} act={act} />
                <Dropdown<string | null>
                  plain
                  ariaLabel={`资料操作：${cur?.name ?? ""}`}
                  value={null}
                  align="end"
                  options={[
                    ...old.map((v) => ({ value: `v${v.version}`, label: `打开第 ${v.version} 版`, group: "旧版本" })),
                    { value: "replace", label: "替换为新版本…" },
                    ...(f.indexed ? [{ value: "unindex", label: "取消索引" }] : []),
                  ]}
                  actions={[{ label: <span className="text-destructive">移除…</span>, onSelect: () => void remove(f) }]}
                  onChange={(v) => {
                    if (v?.startsWith("v")) {
                      const ver = old.find((x) => `v${x.version}` === v);
                      if (ver && !ver.file.deleted) openFile(ver.file.id, toast);
                    } else if (v === "replace") void replace(f);
                    else if (v === "unindex") act(pickyApi.projectFileIndex(f.id, false));
                  }}
                  renderTrigger={() => <Ellipsis size={14} />}
                  triggerClassName="rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                />
              </div>
            );
          })}
        </div>
      </div>
      {library && (
        <AttachmentLibrary
          pickLabel="添加"
          onPick={(a) => {
            void add([a]);
            setLibrary(false);
          }}
          onClose={() => setLibrary(false)}
        />
      )}
      {confirmDialog}
    </Panel>
  );
}

/** The input's 资料 button (conversations in a project): which project files this question uses
 *  in full; the choice stays for the next questions. */
export function FilesButton(props: { files: ProjectFile[]; selected: number[]; onToggle: (id: number) => void; onAdd: () => void; onManage: () => void }) {
  const [open, setOpen] = useState(false);
  const btn = useRef<HTMLButtonElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; bottom: number } | null>(null);
  useLayoutEffect(() => {
    if (!open || !btn.current) return;
    const r = btn.current.getBoundingClientRect();
    setPos({ left: Math.max(8, Math.min(r.left, window.innerWidth - 330)), bottom: window.innerHeight - r.top + 6 });
  }, [open]);
  useEffect(() => {
    if (!open) return;
    const outside = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!pop.current?.contains(t) && !btn.current?.contains(t)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", outside);
    window.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", outside);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);
  const count = props.selected.length;
  const row = "flex w-full items-center gap-2 rounded-control px-2 py-1.5 text-left text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40";
  return (
    <>
      <button ref={btn} type="button" aria-expanded={open} onClick={() => setOpen(!open)} className={`${toolClass} ${open ? "bg-muted text-foreground" : ""}`}>
        <Library size={14} strokeWidth={1.5} /> 资料{count > 0 && ` ${count}`}
      </button>
      {open &&
        createPortal(
          <div
            ref={pop}
            role="dialog"
            aria-label="资料"
            style={{ position: "fixed", left: pos?.left ?? 0, bottom: pos?.bottom ?? 0, visibility: pos ? "visible" : "hidden" }}
            className="z-40 w-80 animate-pop-in rounded-card border bg-popover p-1.5 text-foreground shadow-lg"
          >
            <div className="-m-1 max-h-72 overflow-y-auto p-1">
              {!props.files.length && <p className="px-2 py-1.5 text-sm text-muted-foreground">这个项目还没有资料</p>}
              {props.files.map((f) => {
                const cur = currentFile(f);
                const on = props.selected.includes(f.id);
                return (
                  <button key={f.id} type="button" role="menuitemcheckbox" aria-checked={on} disabled={!usableFile(f)} onClick={() => props.onToggle(f.id)} className={row}>
                    <Check size={14} className={`shrink-0 text-primary ${on ? "" : "invisible"}`} />
                    <span className="min-w-0 flex-1 truncate">{cur?.name ?? "资料"}</span>
                    {cur && <span className="shrink-0 text-xs text-muted-foreground">{fileDetail(cur)}</span>}
                  </button>
                );
              })}
            </div>
            <div className="mx-1 my-1 border-t" />
            <button
              type="button"
              className={row}
              onClick={() => {
                setOpen(false);
                props.onAdd();
              }}
            >
              <span className="w-3.5" /> 添加资料…
            </button>
            <button
              type="button"
              className={row}
              onClick={() => {
                setOpen(false);
                props.onManage();
              }}
            >
              <span className="w-3.5" /> 管理资料…
            </button>
          </div>,
          document.body,
        )}
    </>
  );
}
