// The Markdown-lite box (lite.ts) on LiveText: 念念不忘's new note and in-place edit, the 速记小窗; any
// module wanting the same writing feel uses it (without `tags`, #words are plain text). Grows with the
// text up to 60vh (`grow`: no cap of its own, a box around it scrolls); ⌘↩ submits, Esc cancels;
// ⌘B / ⌘U / ⌘⇧H wrap the selection in ** / ++ / ==; ↩ continues a list, Tab / ⇧Tab indent it; with
// `tags`, typing `#…` lists existing tags at the caret.
import { forwardRef, useMemo } from "react";
import { liteFlavor } from "./lite";
import { LiveText, type LiveTextHandle, type Suggest } from "./LiveText";

export type { LiveTextHandle } from "./LiveText";

const WITH_TAGS = liteFlavor({ tags: true });
const PLAIN = liteFlavor({ tags: false });

/** The `#partial` tag being typed right before the caret, if any. */
function typingTag(value: string, caret: number): { start: number; query: string } | null {
  const m = /(?:^|\s)#([^\s#]*)$/u.exec(value.slice(0, caret));
  if (!m) return null;
  return { start: caret - m[1].length - 1, query: m[1] };
}

function tagSuggest(tags: string[]): Suggest {
  return {
    label: "标签",
    at: typingTag,
    // a complete tag: ↩ is a new line again
    items: (q) =>
      tags.includes(q)
        ? []
        : tags
            .filter((t) => t !== q && t.toLowerCase().includes(q.toLowerCase()))
            .sort((a, b) => Number(!a.startsWith(q)) - Number(!b.startsWith(q)))
            .slice(0, 8),
    show: (t) => `#${t}`,
    insert: (t, after) => `#${t}${after.startsWith(" ") ? "" : " "}`,
  };
}

export const LiteEditor = forwardRef<
  LiveTextHandle,
  {
    value: string;
    onChange: (v: string) => void;
    onSubmit: () => void;
    onCancel?: () => void;
    /** Existing tags: #tags are drawn as tags and suggested while typed. */
    tags?: string[];
    placeholder?: string;
    label?: string;
    autoFocus?: boolean;
    minRows?: number;
    grow?: boolean;
  }
>(function LiteEditor(props, handle) {
  const suggest = useMemo(() => (props.tags ? tagSuggest(props.tags) : undefined), [props.tags]);
  return (
    <LiveText
      ref={handle}
      value={props.value}
      onChange={props.onChange}
      onSubmit={props.onSubmit}
      onCancel={props.onCancel}
      flavor={props.tags ? WITH_TAGS : PLAIN}
      suggest={suggest}
      label={props.label ?? props.placeholder ?? "笔记"}
      placeholder={props.placeholder}
      autoFocus={props.autoFocus}
      style={{ minHeight: `${(props.minRows ?? 3) * 1.625}em` }}
      className={props.grow ? "" : "max-h-[60vh] overflow-y-auto"}
    />
  );
});
