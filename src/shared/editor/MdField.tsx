// A box to write Markdown in that looks like a TextArea (the hand-drawn field, its own scrolling inside): the shared
// live editor with the Markdown flavor (markdown.ts), so headings, lists, **bold** and the rest show as written.
// Every box where I write a prompt uses it (定时任务、今日简报、指令、模板、用途预设、助手的提示词和 SKILL).
// `rows`: a fixed height of that many lines (else the className sizes it, e.g. flex-1); a click in the empty
// space below the text puts the caret at its end. `onBlur` fires when the focus leaves the box.
import { useRef } from "react";
import { LiveText, type LiveTextHandle } from "./LiveText";
import { markdownFlavor } from "./markdown";

const FLAVOR = markdownFlavor();

export function MdField(props: { value: string; onChange: (v: string) => void; label: string; rows?: number; placeholder?: string; autoFocus?: boolean; readOnly?: boolean; onBlur?: () => void; className?: string }) {
  const live = useRef<LiveTextHandle>(null);
  return (
    // the hand-drawn field (like TextArea) is the outer box; the text scrolls inside it, so the edge stays put
    <div
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) props.onBlur?.();
      }}
      style={props.rows ? { height: `calc(${props.rows * 1.625}em + 0.75rem + 2px)` } : undefined}
      className={`pm-field pm-compact pm-area flex w-full cursor-text text-sm ${props.readOnly ? "text-muted-foreground" : ""} ${props.className ?? ""}`}
      data-hand="field"
      data-seed="1413"
    >
      <div
        className="-mx-2.5 -my-1.5 min-h-0 flex-1 overflow-y-auto px-3 py-1.5"
        onMouseDown={(e) => {
          if (e.target !== e.currentTarget || props.readOnly) return;
          e.preventDefault();
          live.current?.focus();
        }}
      >
        <LiveText ref={live} value={props.value} onChange={props.onChange} flavor={FLAVOR} label={props.label} placeholder={props.placeholder} autoFocus={props.autoFocus} readOnly={props.readOnly} focusAt="end" />
      </div>
    </div>
  );
}
