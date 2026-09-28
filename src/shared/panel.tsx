// Right-side editor shell + page-level selection/keyboard helpers shared by module pages.
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { Trash2, X } from "lucide-react";
import { useConfirm } from "./feedback";
import { Button } from "./ui";

/** Overlay editor on the right edge of the page (page root must be `relative`). */
export function SidePanel(props: {
  title: string;
  onClose: () => void;
  onSubmit: (e: FormEvent) => void;
  onDelete?: () => void;
  error?: string | null;
  meta?: ReactNode; // small print under the fields (e.g. 添加于 …)
  children: ReactNode;
  extra?: ReactNode; // rendered outside the form (datalists, dialogs)
}) {
  return (
    <aside className="absolute inset-y-0 right-0 z-10 flex w-[400px] animate-slide-in flex-col border-l bg-card shadow-2xl">
      <form onSubmit={props.onSubmit} className="flex min-h-0 flex-1 flex-col">
        <header data-tauri-drag-region className="flex h-16 shrink-0 items-end justify-between px-6 pb-3">
          <h2 className="pointer-events-none text-base font-bold">{props.title}</h2>
          <button
            type="button"
            aria-label="关闭"
            onClick={props.onClose}
            className="rounded-control p-1 text-muted-foreground hover:bg-muted"
          >
            <X size={16} />
          </button>
        </header>
        <div className="flex-1 space-y-4 overflow-y-auto px-6 py-2">
          {props.children}
          {props.error && <p className="text-sm text-destructive">{props.error}</p>}
          {props.meta && <p className="pt-2 text-xs text-muted-foreground">{props.meta}</p>}
        </div>
        <footer className="flex shrink-0 items-center gap-2 border-t px-6 py-3">
          {props.onDelete && (
            <Button variant="ghost-danger" onClick={props.onDelete}>
              <Trash2 size={14} /> 删除
            </Button>
          )}
          <div className="flex-1" />
          <Button onClick={props.onClose}>取消</Button>
          <Button type="submit" variant="primary">
            保存
          </Button>
        </footer>
      </form>
      {props.extra}
    </aside>
  );
}

/** Which item the side panel shows ("new" = create), guarding unsaved edits. */
export function usePanelSelection<S extends string | number>() {
  const [selection, setSelection] = useState<S | null>(null);
  const dirty = useRef(false);
  const [confirm, confirmDialog] = useConfirm();
  /** Resolves false if the user chose to keep their unsaved edits. */
  const request = async (next: S | null): Promise<boolean> => {
    if (next === selection) return true;
    if (dirty.current) {
      const ok = await confirm({ title: "放弃没有保存的修改？", confirmLabel: "放弃修改", danger: true });
      if (!ok) return false;
    }
    dirty.current = false;
    setSelection(next);
    return true;
  };
  const done = () => {
    dirty.current = false;
    setSelection(null);
  };
  const setDirty = (d: boolean) => {
    dirty.current = d;
  };
  return { selection, request, done, setDirty, confirmDialog };
}

/** ⌘N / ⌘F / Esc for a page. Ignored while a modal dialog is open (it handles its own keys). */
export function usePageKeys(keys: { onNew?: () => void; onFind?: () => void; onEscape?: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector("dialog[open]")) return;
      if (e.metaKey && e.key === "n" && keys.onNew) {
        e.preventDefault();
        keys.onNew();
      } else if (e.metaKey && e.key === "f" && keys.onFind) {
        e.preventDefault();
        keys.onFind();
      } else if (e.key === "Escape" && keys.onEscape) {
        keys.onEscape();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
}
