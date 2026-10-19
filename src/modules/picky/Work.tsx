// Work in conversations (rules in Rust work.rs; the plugin library's Word 工具箱 / 文本工具箱 propose):
// the reply's card — each changed paragraph as a diff with its tick, 写入文件 writes the ticked ones into
// the file (refused when it changed meanwhile); or a new Word document (name, folder, its text), 保存
// writes it. An older question shows the file it picked (EditFileTag). 保存为 Word — any finished reply
// into a .docx (the cited sources at the end) in a folder chosen here (remembered), then 打开 / 在访达中显示.
import { useEffect, useState, type FormEvent } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FilePen, FilePlus2 } from "lucide-react";
import { errorText, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { Modal } from "@/shared/Modal";
import { Button, Field, TextInput } from "@/shared/ui";
import { MarkdownView } from "./MarkdownView";
import { DiffView } from "./Assist";
import { pickyApi, type EditTarget, type FileEdit } from "./api";

const open = (path: string, toast: ReturnType<typeof useToast>) => void ipc.openLink(path).catch((e) => toast(errorText(e), "error"));
const folderOf = (path: string) => path.slice(0, path.lastIndexOf("/")) || "/";
const home = (p: string) => p.replace(/^\/Users\/[^/]+/, "~");

/** Above a question: the file it asked to change. */
export function EditFileTag(props: { target: EditTarget }) {
  const toast = useToast();
  return (
    <button
      type="button"
      data-find-skip
      onClick={() => open(props.target.path, toast)}
      className="mb-1.5 flex max-w-[85%] items-center gap-1.5 rounded-control bg-primary/10 px-2 py-1 text-xs outline-none hover:bg-primary/15 focus-visible:ring-2 focus-visible:ring-ring"
    >
      <FilePen size={12} strokeWidth={1.75} className="shrink-0" />
      <span className="text-muted-foreground">改</span>
      <span className="truncate">{props.target.name}</span>
    </button>
  );
}

/** Under a reply: a new Word document it proposes — its name, folder and text; 保存 writes it. */
function NewWordCard(props: { edit: FileEdit; streaming: boolean; onChanged: () => void }) {
  const { edit } = props;
  const toast = useToast();
  const [name, setName] = useState(edit.name.replace(/\.docx$/, ""));
  const [folder, setFolder] = useState(edit.path);
  const [busy, setBusy] = useState(false);
  const pending = edit.state === "pending";
  const content = edit.items[0]?.after ?? "";
  const pick = async () => {
    const p = await openDialog({ directory: true, multiple: false, title: "保存到", defaultPath: folder || undefined });
    if (typeof p === "string") setFolder(p);
  };
  const decide = async (save: boolean) => {
    setBusy(true);
    try {
      await (save ? pickyApi.fileCreateConfirm(edit.runId, folder, name) : pickyApi.fileEditCancel(edit.runId));
      props.onChanged();
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <div data-find-skip className="mt-2 max-w-2xl rounded-card border bg-card text-sm">
      <p className="flex items-center gap-1.5 border-b px-3 py-2 text-xs text-muted-foreground">
        <FilePlus2 size={12} strokeWidth={1.75} /> 新建 Word「{pending ? `${name}.docx` : edit.name}」
      </p>
      <div className="-m-1 max-h-60 overflow-y-auto p-1">
        <div className="px-3 py-2">
          <MarkdownView source={content} />
        </div>
      </div>
      <div className="flex flex-wrap items-center justify-end gap-2 border-t px-3 py-2">
        {pending ? (
          <>
            <TextInput aria-label="文件名" value={name} maxLength={80} onChange={(e) => setName(e.target.value)} className="w-48 py-1" />
            <p className="min-w-0 flex-1 truncate text-xs text-muted-foreground" title={folder}>
              {home(folder)}
            </p>
            <Button className="py-1" disabled={busy} onClick={() => void pick()}>
              选择…
            </Button>
            <Button disabled={busy || props.streaming} onClick={() => void decide(false)}>
              取消
            </Button>
            <Button variant="primary" disabled={busy || props.streaming || !name.trim()} onClick={() => void decide(true)}>
              保存
            </Button>
          </>
        ) : edit.state === "written" ? (
          <>
            <span className="selectable min-w-0 flex-1 truncate text-xs text-muted-foreground" title={edit.path}>
              已保存：{home(edit.path)}
            </span>
            <Button className="py-1" onClick={() => open(edit.path, toast)}>
              打开
            </Button>
            <Button className="py-1" onClick={() => open(folderOf(edit.path), toast)}>
              在访达中显示
            </Button>
          </>
        ) : (
          <span className="text-xs text-muted-foreground">已取消</span>
        )}
      </div>
    </div>
  );
}

/** Under a reply: the changes it proposes to a file (or a new Word document). */
export function EditCard(props: { edit: FileEdit; streaming: boolean; onChanged: () => void }) {
  if (props.edit.kind === "new-word") return <NewWordCard {...props} />;
  return <ChangesCard {...props} />;
}

function ChangesCard(props: { edit: FileEdit; streaming: boolean; onChanged: () => void }) {
  const { edit } = props;
  const toast = useToast();
  const [off, setOff] = useState<Set<number>>(() => new Set());
  const [busy, setBusy] = useState(false);
  const pending = edit.state === "pending";
  const ticked = edit.items.filter((i) => !off.has(i.position));
  const decide = async (write: boolean) => {
    setBusy(true);
    try {
      await (write ? pickyApi.fileEditConfirm(edit.runId, ticked.map((i) => i.position)) : pickyApi.fileEditCancel(edit.runId));
      props.onChanged();
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setBusy(false);
    }
  };
  if (!edit.items.length) return null;
  return (
    <div data-find-skip className="mt-2 max-w-2xl rounded-card border bg-card text-sm">
      <p className="border-b px-3 py-2 text-xs text-muted-foreground">
        改「{edit.name}」· {edit.items.length} 段
      </p>
      <ul className="divide-y">
        {edit.items.map((it) => (
          <li key={it.position} className={`flex items-start gap-2 px-3 py-2 ${!pending && !it.checked ? "text-muted-foreground" : ""}`}>
            <input
              type="checkbox"
              checked={pending ? !off.has(it.position) : it.checked}
              disabled={!pending || busy}
              onChange={() => setOff((s) => (s.has(it.position) ? new Set([...s].filter((x) => x !== it.position)) : new Set(s).add(it.position)))}
              aria-label={`第 ${it.paragraph} 段`}
              className="mt-1 size-3.5 shrink-0 accent-[var(--primary)]"
            />
            <div className="min-w-0 flex-1">
              <p className="mb-1 text-xs text-muted-foreground">第 {it.paragraph} 段</p>
              {it.after ? <DiffView from={it.before} to={it.after} /> : <p className="rounded-control bg-background px-2.5 py-2 leading-relaxed text-muted-foreground line-through">{it.before}</p>}
            </div>
          </li>
        ))}
      </ul>
      <div className="flex items-center justify-end gap-2 border-t px-3 py-2">
        {pending ? (
          <>
            <Button disabled={busy || props.streaming} onClick={() => void decide(false)}>
              取消
            </Button>
            <Button variant="primary" disabled={busy || props.streaming || !ticked.length} onClick={() => void decide(true)}>
              写入文件
            </Button>
          </>
        ) : edit.state === "written" ? (
          <>
            <span className="text-xs text-muted-foreground">已写入</span>
            <Button className="py-1" onClick={() => open(edit.path, toast)}>
              打开文件
            </Button>
          </>
        ) : (
          <span className="text-xs text-muted-foreground">已取消</span>
        )}
      </div>
    </div>
  );
}

/** 保存为 Word: the name and folder (remembered), then where it went. */
export function WordDialog(props: { messageId: number; onClose: () => void }) {
  const toast = useToast();
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("");
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    pickyApi.wordDefaults(props.messageId).then(
      (d) => {
        setName(d.name);
        setFolder(d.folder);
      },
      (e) => setError(errorText(e)),
    );
  }, [props.messageId]);
  const pick = async () => {
    const p = await openDialog({ directory: true, multiple: false, title: "保存到", defaultPath: folder || undefined });
    if (typeof p === "string") setFolder(p);
  };
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (saved) return props.onClose();
    setError(null);
    try {
      setSaved(await pickyApi.saveWord(props.messageId, folder, name));
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal title="保存为 Word" onClose={props.onClose} onSubmit={submit} error={error} submitLabel={saved ? "完成" : "保存"} wide>
      {saved ? (
        <div className="space-y-3 text-sm">
          <p className="selectable break-all">{saved}</p>
          <div className="flex gap-2">
            <Button onClick={() => open(saved, toast)}>打开</Button>
            <Button onClick={() => open(folderOf(saved), toast)}>在访达中显示</Button>
          </div>
        </div>
      ) : (
        <>
          <Field label="文件名">
            <TextInput autoFocus aria-label="文件名" value={name} maxLength={80} onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field label="保存到">
            <div className="flex items-center gap-2">
              <p className="selectable min-w-0 flex-1 truncate text-sm" title={folder}>
                {folder.replace(/^\/Users\/[^/]+/, "~")}
              </p>
              <Button onClick={() => void pick()}>选择…</Button>
            </div>
          </Field>
        </>
      )}
    </Modal>
  );
}
