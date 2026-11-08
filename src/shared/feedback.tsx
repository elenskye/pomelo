// Toasts + promise-based confirm dialog (native <dialog>: focus trap + Esc for free;
// showModal focuses the first button = 取消, a safe default for destructive confirms).
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { Button } from "./ui";

type ToastKind = "info" | "error";
type ToastItem = { id: number; text: string; kind: ToastKind; leaving: boolean };
const ToastCtx = createContext<(text: string, kind?: ToastKind) => void>(() => {});

/** Toasts are sticky notes (Design System Toast): one slaps on in the bottom-right corner, stays a moment and
 *  peels itself off (motion part toast; an error's note is pink and stays longer). */
export function ToastProvider(props: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const push = useCallback((text: string, kind: ToastKind = "info") => {
    const id = Date.now() + Math.random();
    setItems((xs) => [...xs, { id, text, kind, leaving: false }]);
    const stay = kind === "error" ? 6000 : 2600;
    setTimeout(() => setItems((xs) => xs.map((x) => (x.id === id ? { ...x, leaving: true } : x))), stay);
    setTimeout(() => setItems((xs) => xs.filter((x) => x.id !== id)), stay + 400);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {props.children}
      <div className="pointer-events-none fixed right-6 bottom-6 z-50 flex flex-col items-end gap-3">
        {items.map((t, i) => (
          <div
            key={t.id}
            role="status"
            data-hand="note"
            data-seed={1450 + (i % 3)}
            data-tilt={i % 2 ? 2 : -3}
            data-tapes="1"
            className={`pm-toast toast ${t.kind === "error" ? "is-error" : ""} ${t.leaving ? "is-leaving" : "is-in"}`}
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
