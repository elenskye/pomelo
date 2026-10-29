// A choice from a list that keeps growing (jobs, questions, materials, companies…), where a dropdown
// gets too long: the field shows the current choice; a click opens a dialog with a search box and the
// list (grouped), and a click (or Enter on the first one found) picks. The dialog has no <form> (it may
// sit inside another dialog's form) and renders at <body>; its clicks don't reach the page behind.
import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { ChevronsUpDown, X } from "lucide-react";
import { IconButton, SearchInput } from "./ui";

export type PickOption<T> = { value: T; label: string; detail?: string; group?: string };

export function PickField<T extends string | number | null>(props: {
  ariaLabel: string;
  /** The dialog's title (default: ariaLabel). */
  title?: string;
  value: T;
  options: PickOption<T>[];
  placeholder?: string;
  onChange: (v: T) => void;
  disabled?: boolean;
  /** Full width (form fields). */
  block?: boolean;
  /** Its own trigger content (e.g. 「＋ 加一道」), drawn plain. */
  trigger?: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const current = props.options.find((o) => o.value === props.value);
  const look = props.trigger
    ? "inline-flex shrink-0 items-center rounded-control px-1 py-0.5 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
    : `${props.block ? "flex w-full min-w-0 justify-between" : "inline-flex max-w-full shrink-0"} items-center gap-1.5 rounded-control border bg-card py-1.5 pr-2 pl-3 text-sm whitespace-nowrap transition-colors duration-150 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50`;
  return (
    <>
      <button type="button" aria-label={props.ariaLabel} aria-haspopup="dialog" disabled={props.disabled} onClick={() => setOpen(true)} className={look}>
        {props.trigger ?? (
          <>
            <span className={`min-w-0 truncate text-left ${current ? "" : "text-muted-foreground"}`} title={current?.label}>
              {current?.label ?? props.placeholder ?? "请选择"}
            </span>
            <ChevronsUpDown size={14} className="shrink-0 text-muted-foreground" />
          </>
        )}
      </button>
      {open &&
        createPortal(
          <PickDialog<T>
            title={props.title ?? props.ariaLabel}
            value={props.value}
            options={props.options}
            onPick={(v) => {
              setOpen(false);
              props.onChange(v);
            }}
            onClose={() => setOpen(false)}
          />,
          document.body,
        )}
    </>
  );
}

function PickDialog<T>(props: { title: string; value: T; options: PickOption<T>[]; onPick: (v: T) => void; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [query, setQuery] = useState("");
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
    // showModal focuses the first button (关闭); typing should go to the search box
    d?.querySelector<HTMLInputElement>("input[type=search]")?.focus();
  }, []);
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = props.options.filter((o) => words.every((w) => `${o.label} ${o.detail ?? ""} ${o.group ?? ""}`.toLowerCase().includes(w)));
  const groups = [...new Set(shown.map((o) => o.group ?? ""))];
  return (
    <dialog
      ref={ref}
      aria-label={props.title}
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => e.stopPropagation()}
      className="m-auto h-fit max-h-[85vh] w-[560px] max-w-[calc(100vw-48px)] rounded-card border bg-popover p-5 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <div className="flex items-center gap-2 pb-3">
        <p className="min-w-0 flex-1 truncate text-base font-bold">{props.title}</p>
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      <SearchInput
        autoFocus
        aria-label="找"
        placeholder="找"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && shown.length) {
            e.preventDefault();
            props.onPick(shown[0].value);
          }
        }}
      />
      <div role="listbox" aria-label={props.title} className="-m-1 mt-3 max-h-[56vh] space-y-3 overflow-y-auto p-1">
        {!shown.length && <p className="py-6 text-center text-sm text-muted-foreground">没有对得上的</p>}
        {groups.map((g) => (
          <section key={g} className="space-y-0.5">
            {g && <p className="px-1 pb-0.5 text-xs text-muted-foreground">{g}</p>}
            {shown
              .filter((o) => (o.group ?? "") === g)
              .map((o, i) => {
                const on = o.value === props.value;
                return (
                  <button
                    key={`${String(o.value)}-${i}`}
                    type="button"
                    role="option"
                    aria-selected={on}
                    onClick={() => props.onPick(o.value)}
                    className={`flex w-full items-center gap-3 rounded-control border px-3 py-2 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "border-primary bg-accent text-accent-foreground" : "border-transparent hover:bg-muted"}`}
                  >
                    <span className="min-w-0 flex-1 truncate" title={o.label}>
                      {o.label}
                    </span>
                    {o.detail && <span className="max-w-[45%] shrink-0 truncate text-xs text-muted-foreground">{o.detail}</span>}
                  </button>
                );
              })}
          </section>
        ))}
      </div>
    </dialog>
  );
}
