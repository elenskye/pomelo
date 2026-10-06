// Note text box (new note + editing in place): grows with the text; ⌘↩ saves, Esc cancels;
// ⌘B / ⌘U / ⌘⇧H wrap the selection in ** / ++ / ==; ↩ continues a list, Tab / ⇧Tab indent it;
// typing `#…` suggests existing tags. Never acts while an IME is composing.
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";

export type EditorHandle = { focus: () => void };

const LIST = /^( *)(?:([-*])|(\d+)([.)])) (.*)$/;

/** Replace [start, end) with text, keeping the textarea's own undo where WebKit allows. */
function replace(el: HTMLTextAreaElement, start: number, end: number, text: string, caret?: [number, number]) {
  el.focus();
  el.setSelectionRange(start, end);
  // WebKit: an empty insertText leaves the caret before a preceding newline; delete doesn't
  const ok = text === "" ? start === end || document.execCommand("delete") : document.execCommand("insertText", false, text);
  if (!ok) {
    el.setRangeText(text, start, end, "end");
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }
  if (caret) el.setSelectionRange(caret[0], caret[1]);
}

function wrap(el: HTMLTextAreaElement, mark: string) {
  const { selectionStart: s, selectionEnd: e, value } = el;
  const n = mark.length;
  if (value.slice(s - n, s) === mark && value.slice(e, e + n) === mark) {
    replace(el, s - n, e + n, value.slice(s, e), [s - n, e - n]); // already wrapped → unwrap
  } else {
    replace(el, s, e, mark + value.slice(s, e) + mark, [s + n, e + n]);
  }
}

const lineStart = (v: string, i: number) => v.lastIndexOf("\n", i - 1) + 1;

/** The `#partial` tag being typed right before the caret, if any. */
function typingTag(value: string, caret: number): { start: number; text: string } | null {
  const m = /(?:^|\s)#([^\s#]*)$/u.exec(value.slice(0, caret));
  if (!m) return null;
  return { start: caret - m[1].length - 1, text: m[1] };
}

export const Editor = forwardRef<
  EditorHandle,
  {
    value: string;
    onChange: (v: string) => void;
    onSubmit: () => void;
    onCancel?: () => void;
    tags: string[];
    placeholder?: string;
    autoFocus?: boolean;
    minRows?: number;
  }
>(function Editor(props, handle) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const [caret, setCaret] = useState(0);
  const [focused, setFocused] = useState(false);
  const [active, setActive] = useState(0);
  const [dismissed, setDismissed] = useState<number | null>(null); // caret where Esc closed the list

  useImperativeHandle(handle, () => ({ focus: () => ref.current?.focus() }), []);

  // grow with the text (capped by CSS max-height)
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight + 2}px`;
  }, [props.value]);

  useEffect(() => {
    const el = ref.current;
    if (props.autoFocus && el) {
      el.focus();
      el.setSelectionRange(el.value.length, el.value.length);
    }
  }, [props.autoFocus]);

  const typing = focused ? typingTag(props.value, caret) : null;
  const suggestions =
    typing && dismissed !== caret && !props.tags.includes(typing.text) // a complete tag: ↩ is a new line again
      ? props.tags
          .filter((t) => t !== typing.text && t.toLowerCase().includes(typing.text.toLowerCase()))
          .sort((a, b) => Number(!a.startsWith(typing.text)) - Number(!b.startsWith(typing.text)))
          .slice(0, 8)
      : [];
  const pick = (tag: string) => {
    const el = ref.current;
    if (!el || !typing) return;
    const next = props.value[caret] === " " ? "" : " ";
    replace(el, typing.start, caret, `#${tag}${next}`);
    setCaret(el.selectionStart);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;
    const el = e.currentTarget;
    if (suggestions.length > 0) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setActive((a) => (a + (e.key === "ArrowDown" ? 1 : -1) + suggestions.length) % suggestions.length);
        return;
      }
      if ((e.key === "Enter" && !e.metaKey) || e.key === "Tab") {
        e.preventDefault();
        pick(suggestions[Math.min(active, suggestions.length - 1)]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setDismissed(caret);
        return;
      }
    }
    if (e.metaKey && e.key === "Enter") {
      e.preventDefault();
      props.onSubmit();
      return;
    }
    if (e.key === "Escape" && props.onCancel) {
      e.preventDefault();
      e.stopPropagation();
      props.onCancel();
      return;
    }
    const k = e.key.toLowerCase();
    if (e.metaKey && !e.altKey && ((k === "b" && !e.shiftKey) || (k === "u" && !e.shiftKey) || (k === "h" && e.shiftKey))) {
      e.preventDefault();
      wrap(el, k === "b" ? "**" : k === "u" ? "++" : "==");
      return;
    }
    const { selectionStart: s, selectionEnd: end, value } = el;
    const ls = lineStart(value, s);
    const le = value.indexOf("\n", ls) < 0 ? value.length : value.indexOf("\n", ls);
    const li = LIST.exec(value.slice(ls, le));
    if (e.key === "Enter" && !e.shiftKey && !e.metaKey && li && s === end) {
      e.preventDefault();
      if (!li[5].trim()) {
        if (li[1].length >= 2) replace(el, ls, ls + 2, "", [s - 2, s - 2]); // empty nested item: one level up
        else replace(el, ls, le, ""); // empty top item: end the list
      } else {
        const marker = li[2] ? `${li[2]} ` : `${Number(li[3]) + 1}${li[4]} `;
        replace(el, s, s, `\n${li[1]}${marker}`);
      }
      return;
    }
    if (e.key === "Tab" && li) {
      e.preventDefault();
      if (e.shiftKey) {
        if (li[1].length >= 2) replace(el, ls, ls + 2, "", [s - 2, end - 2]);
      } else replace(el, ls, ls, "  ", [s + 2, end + 2]);
    }
  };

  return (
    <div className="relative">
      <textarea
        ref={ref}
        value={props.value}
        rows={props.minRows ?? 3}
        spellCheck={false}
        placeholder={props.placeholder}
        onChange={(e) => {
          props.onChange(e.target.value);
          setCaret(e.target.selectionStart);
          setActive(0);
        }}
        onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        onKeyDown={onKeyDown}
        className="block max-h-[60vh] w-full resize-none bg-transparent leading-relaxed outline-none placeholder:text-muted-foreground"
      />
      {suggestions.length > 0 && (
        <div
          role="listbox"
          aria-label="标签"
          onMouseDown={(e) => e.preventDefault()}
          className="absolute top-full left-0 z-30 mt-1 max-w-full min-w-48 animate-pop-in rounded-card border bg-popover p-1 shadow-lg"
        >
          {suggestions.map((t, i) => (
            <div
              key={t}
              role="option"
              aria-selected={i === active}
              onMouseEnter={() => setActive(i)}
              onClick={() => pick(t)}
              className={`cursor-default truncate rounded-md px-3 py-1 text-sm ${i === active ? "bg-muted" : ""}`}
            >
              #{t}
            </div>
          ))}
        </div>
      )}
    </div>
  );
});
