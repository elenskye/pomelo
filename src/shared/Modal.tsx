// Centered form dialog (native <dialog>, Esc = cancel). Footer: optional left slot, 取消, submit.
import { useEffect, useRef, type FormEvent, type ReactNode } from "react";
import { Button } from "./ui";

export function Modal(props: {
  title: string;
  onClose: () => void;
  onSubmit: (e: FormEvent) => void;
  error: string | null;
  children: ReactNode;
  footerLeft?: ReactNode;
  submitLabel: string;
  submitDisabled?: boolean;
  extra?: ReactNode;
  wide?: boolean;
  /** "xl": 720 wide (editors with rows of fields, diffs). */
  size?: "xl";
}) {
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
      className={`m-auto h-fit max-h-[85vh] ${props.size === "xl" ? "w-[720px] max-w-[calc(100vw-48px)]" : props.wide ? "w-[560px]" : "w-[440px]"} rounded-card border bg-popover p-5 text-foreground shadow-xl backdrop:bg-black/30`}
    >
      <form onSubmit={props.onSubmit} className="space-y-4">
        <p className="text-base font-bold">{props.title}</p>
        {props.children}
        {props.error && <p className="text-sm text-destructive">{props.error}</p>}
        <div className="flex items-center gap-2 pt-1">
          {props.footerLeft}
          <div className="flex-1" />
          <Button onClick={props.onClose}>取消</Button>
          <Button type="submit" variant="primary" disabled={props.submitDisabled}>
            {props.submitLabel}
          </Button>
        </div>
      </form>
      {props.extra}
    </dialog>
  );
}
