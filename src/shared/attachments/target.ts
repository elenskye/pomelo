// Where dropped and pasted files go: the attachment target they land in (the note being written, the
// open task panel…) — the innermost registered element under the drop point, or holding the focus for
// a paste. The app's window delivers drops as paths (Tauri); the browser mock delivers File objects.
// A window drop that hits no target goes to the latest one still on screen (the note being edited,
// the open panel, else the box for a new note).
import { useEffect, useRef, useState, type RefObject } from "react";
import { attachmentsApi, type FileInfo } from "./api";

type Target = { el: RefObject<HTMLElement | null>; add: (files: Promise<FileInfo[]>) => void; hover: (on: boolean) => void };

const targets: Target[] = [];
let hovered: Target | null = null;
let installed = false;

/** The innermost target containing `node` (latest registered wins between nested equals). */
function targetOf(node: Node | null): Target | null {
  let best: Target | null = null;
  for (const t of targets) {
    const el = t.el.current;
    if (el && node && el.contains(node) && (!best || best.el.current!.contains(el))) best = t;
  }
  return best;
}

function setHovered(t: Target | null) {
  if (hovered === t) return;
  hovered?.hover(false);
  hovered = t;
  t?.hover(true);
}

function fromFiles(files: File[]): Promise<FileInfo[]> {
  return Promise.all(files.map((f) => attachmentsApi.addFile(f)));
}

function install() {
  if (installed) return;
  installed = true;
  import("@tauri-apps/api/webview")
    .then(({ getCurrentWebview }) =>
      getCurrentWebview().onDragDropEvent((e) => {
        const d = e.payload;
        if (d.type === "leave") return setHovered(null);
        const r = window.devicePixelRatio || 1;
        const t = targetOf(document.elementFromPoint(d.position.x / r, d.position.y / r)) ?? [...targets].reverse().find((x) => x.el.current?.isConnected) ?? null;
        setHovered(d.type === "drop" ? null : t);
        if (d.type === "drop" && t && d.paths.length) t.add(attachmentsApi.addPaths(d.paths));
      }),
    )
    .catch(() => {});
  document.addEventListener("dragover", (e) => {
    if (!e.dataTransfer?.types.includes("Files")) return;
    const t = targetOf(e.target as Node);
    setHovered(t);
    if (t) e.preventDefault();
  });
  document.addEventListener("dragleave", (e) => {
    if (!e.relatedTarget) setHovered(null);
  });
  document.addEventListener("drop", (e) => {
    const files = [...(e.dataTransfer?.files ?? [])];
    const t = targetOf(e.target as Node);
    setHovered(null);
    if (!files.length || !t) return;
    e.preventDefault();
    t.add(fromFiles(files));
  });
  // capture: before an editable box would paste the file as text
  document.addEventListener(
    "paste",
    (e) => {
      const files = [...(e.clipboardData?.files ?? [])];
      const t = files.length ? targetOf(document.activeElement) : null;
      if (!t) return;
      e.preventDefault();
      t.add(fromFiles(files));
    },
    true,
  );
}

/** Makes `el` take dropped and pasted files; returns whether files are being dragged over it. */
export function useAttachTarget(el: RefObject<HTMLElement | null>, onFiles: (files: FileInfo[]) => void, onError: (e: unknown) => void, enabled = true) {
  const [dropping, setDropping] = useState(false);
  const cb = useRef({ onFiles, onError });
  cb.current = { onFiles, onError };
  useEffect(() => {
    if (!enabled) return;
    install();
    const t: Target = { el, add: (p) => void p.then((fs) => cb.current.onFiles(fs), (e) => cb.current.onError(e)), hover: setDropping };
    targets.push(t);
    return () => {
      targets.splice(targets.indexOf(t), 1);
      if (hovered === t) hovered = null;
    };
  }, [el, enabled]);
  return dropping;
}
