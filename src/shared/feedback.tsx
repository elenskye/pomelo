// Toasts + promise-based confirm dialog (native <dialog>: focus trap + Esc for free;
// showModal focuses the first button = 取消, a safe default for destructive confirms).
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { Button } from "./ui";

type ToastKind = "info" | "error";
type ToastItem = { id: number; text: string; kind: ToastKind };
const ToastCtx = createContext<(text: string, kind?: ToastKind) => void>(() => {});

export function ToastProvider(props: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const push = useCallback((text: string, kind: ToastKind = "info") => {
    const id = Date.now() + Math.random();
    setItems((xs) => [...xs, { id, text, kind }]);
    setTimeout(() => setItems((xs) => xs.filter((x) => x.id !== id)), kind === "error" ? 6000 : 2500);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {props.children}
      <div className="pointer-events-none fixed bottom-6 left-1/2 z-50 flex -translate-x-1/2 flex-col items-center gap-2">
        {items.map((t) => (
          <div
            key={t.id}
            role="status"
            className={`max-w-md rounded-control px-4 py-2 text-sm shadow-lg ${
              t.kind === "error" ? "border bg-card text-destructive" : "bg-foreground text-background"
            }`}
          >
            {t.text}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

export const useToast = () => useContext(ToastCtx);

/** Stringify a rejected invoke (Rust returns Chinese error strings). */
export const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

type ConfirmOptions = { title: string; message?: ReactNode; confirmLabel?: string; danger?: boolean };

/** `const [confirm, confirmDialog] = useConfirm();` render `{confirmDialog}`, then `if (await confirm({...}))`. */
export function useConfirm() {
  const [req, setReq] = useState<(ConfirmOptions & { resolve: (ok: boolean) => void }) | null>(null);
  const confirm = useCallback(
    (o: ConfirmOptions) => new Promise<boolean>((resolve) => setReq({ ...o, resolve })),
    [],
  );
  const dialog = req ? (
    <ConfirmDialog
      {...req}
      onDone={(ok) => {
        req.resolve(ok);
        setReq(null);
      }}
    />
  ) : null;
  return [confirm, dialog] as const;
}

function ConfirmDialog(props: ConfirmOptions & { onDone: (ok: boolean) => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal(); // guard: StrictMode runs effects twice
  }, []);
  return (
    <dialog
      ref={ref}
      onCancel={(e) => {
        e.preventDefault();
        props.onDone(false);
      }}
      className="m-auto h-fit w-[380px] rounded-card border bg-popover p-5 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <p className="text-base font-bold">{props.title}</p>
      {props.message && <div className="mt-2 text-sm leading-relaxed text-muted-foreground">{props.message}</div>}
      <div className="mt-5 flex justify-end gap-2">
        <Button onClick={() => props.onDone(false)}>取消</Button>
        <Button variant={props.danger ? "danger" : "primary"} onClick={() => props.onDone(true)}>
          {props.confirmLabel ?? "确定"}
        </Button>
      </div>
    </dialog>
  );
}
