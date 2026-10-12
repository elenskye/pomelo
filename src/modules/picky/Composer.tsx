// The input: Enter sends, Shift+Enter is a new line, Enter while composing Chinese only confirms.
// Drafts are kept per conversation while the app runs (not saved: gone after quitting). While a reply is
// being written the next question can be typed; sending waits until it's done (the button stops it).
// Templates (Picky's rules, slots.ts): a draft that is just "/filter" opens the "/" menu (↑↓ Enter, Esc
// closes it for that text); choosing replaces the draft, the 模板 menu and the empty page insert at the
// cursor. `{{名称}}` places are tinted (a mirror behind the transparent textarea) and Tab / Shift-Tab
// step through them. Inserting goes through the editor's own insert so ⌘Z in the field still works.
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ArrowUp, FileText, Square } from "lucide-react";
import { Dropdown, type DropdownOption } from "@/shared/Dropdown";
import { TEMPLATES_KEY, pickyApi, type Template } from "./api";
import { nextSlot, slashMatches, slashQuery, slots } from "./slots";

const drafts = new Map<string, string>();

export type ComposerHandle = {
  /** Inserts a template's body (replacing a "/" draft) and selects its first place to fill. */
  insert: (t: Template) => void;
  send: () => void;
  text: () => string;
  /** Replaces the draft (after /记住 was saved, the rest of it). */
  setText: (t: string) => void;
  focus: () => void;
};

/** 「/记住」 in the "/" menu once a letter follows "/" (a bare "/" keeps templates first). */
const REMEMBER: Template = { id: -1, title: "记住", category: "保存为记忆", body: "/记住 ", pinned: false, lastUsedAt: null };
const commandsFor = (q: string) => (q && ("记住".startsWith(q) || "jizhu".startsWith(q.toLowerCase())) ? [REMEMBER] : []);

const toolClass =
  "flex items-center gap-1 rounded-control px-2 py-1 text-xs text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40";

export const Composer = forwardRef<
  ComposerHandle,
  {
    /** Which conversation the draft belongs to ("new" = not saved yet). */
    draftKey: string;
    busy: boolean;
    templates: Template[];
    onManageTemplates: () => void;
    /** More buttons in the bottom row: before 模板 (attachments) and after it (指令, 资料). */
    before?: ReactNode;
    tools?: ReactNode;
    /** Files going with the question (shown above the text). */
    chips?: ReactNode;
    /** The question may be empty (it has attachments). */
    allowEmpty?: boolean;
    /** Files are being dragged over the window. */
    dropping?: boolean;
    onTextChange?: (text: string) => void;
    /** Resolves false when the question was not sent (the text comes back). */
    onSend: (text: string) => Promise<boolean>;
    onStop: () => void;
  }
>(function Composer(props, ref) {
  const qc = useQueryClient();
  const [text, setText] = useState(() => drafts.get(props.draftKey) ?? "");
  const [sel, setSel] = useState<[number, number]>([0, 0]);
  /** The "/" menu was closed (Esc) for exactly this text. */
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const area = useRef<HTMLTextAreaElement>(null);
  const mirror = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const pendingSel = useRef<[number, number] | null>(null);
  const onText = useRef(props.onTextChange);
  onText.current = props.onTextChange;

  useEffect(() => {
    setText(drafts.get(props.draftKey) ?? "");
    setDismissed(null);
  }, [props.draftKey]);
  useEffect(() => onText.current?.(text), [text]);

  const change = (v: string) => {
    setText(v);
    setActive(0);
    if (v) drafts.set(props.draftKey, v);
    else drafts.delete(props.draftKey);
  };

  const query = dismissed === text ? null : slashQuery(text);
  const found = query == null ? null : slashMatches(props.templates, query);
  const commands = query == null ? [] : commandsFor(query);
  const items = found ? [...commands, ...found.pinned, ...found.others] : [];

  const send = async () => {
    const t = text.trim();
    if ((!t && !props.allowEmpty) || props.busy) return;
    const key = props.draftKey;
    change("");
    if (!(await props.onSend(t)) && !drafts.get(key)) {
      drafts.set(key, text);
      if (key === props.draftKey) setText(text);
    }
  };

  const select = (a: number, b: number) => {
    const el = area.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(a, b);
    setSel([a, b]);
  };

  const insert = (t: Template) => {
    const el = area.current;
    if (!el) return;
    const replacing = query != null;
    const [a, b] = replacing ? [0, text.length] : [el.selectionStart, el.selectionEnd];
    const next = text.slice(0, a) + t.body + text.slice(b);
    const first = slots(next).find((s) => s.start >= a && s.start < a + t.body.length);
    pendingSel.current = first ? [first.start, first.end] : [a + t.body.length, a + t.body.length];
    el.focus();
    el.setSelectionRange(a, b);
    // the editor's own insert keeps ⌘Z working in the field; fall back to setting the text
    if (!document.execCommand("insertText", false, t.body) || el.value !== next) change(next);
    setDismissed(null);
    if (t.id < 0) return;
    void pickyApi.templateUsed(t.id).then(
      () => qc.invalidateQueries({ queryKey: TEMPLATES_KEY }),
      () => {},
    );
  };

  useImperativeHandle(ref, () => ({ insert, send: () => void send(), text: () => text, setText: change, focus: () => area.current?.focus() }));

  // after an insert: select the first place to fill
  useLayoutEffect(() => {
    const p = pendingSel.current;
    if (!p) return;
    pendingSel.current = null;
    select(p[0], p[1]);
  }, [text]); // eslint-disable-line react-hooks/exhaustive-deps

  // grows with its text (up to ~8 lines)
  useLayoutEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
    if (mirror.current) mirror.current.scrollTop = el.scrollTop;
  }, [text]);

  useEffect(() => {
    menu.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active, query]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const composing = e.nativeEvent.isComposing || e.keyCode === 229;
    if (composing) return;
    if (query != null) {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setDismissed(text);
        return;
      }
      if (items.length && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
        e.preventDefault();
        setActive((i) => (i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length);
        return;
      }
      if (items.length && e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        insert(items[Math.min(active, items.length - 1)]);
        return;
      }
    }
    if (e.key === "Tab" && !e.altKey && !e.metaKey && !e.ctrlKey) {
      const el = e.currentTarget;
      const s = nextSlot(slots(text), el.selectionStart, el.selectionEnd, e.shiftKey);
      if (s) {
        e.preventDefault();
        select(s.start, s.end);
      }
      return;
    }
    if (e.key !== "Enter" || e.shiftKey) return;
    e.preventDefault();
    void send();
  };

  const marks = slots(text);
  const menuOptions: DropdownOption<number>[] = [
    ...props.templates.filter((t) => t.pinned).map((t) => ({ value: t.id, label: t.title, group: "置顶" })),
    ...props.templates.filter((t) => !t.pinned).map((t) => ({ value: t.id, label: t.category ? `${t.title}（${t.category}）` : t.title, group: "全部" })),
  ];

  return (
    <div className="relative">
      {query != null && (
        <div ref={menu} role="listbox" aria-label="模板" className="absolute bottom-full left-0 z-30 mb-2 max-h-72 w-80 overflow-y-auto rounded-card border bg-popover p-1.5 text-sm shadow-lg">
          {!items.length && <p className="px-2 py-1.5 text-muted-foreground">{props.templates.length ? "没有匹配的模板" : "暂无模板"}</p>}
          {found &&
            (
              [
                ["命令", commands, 0],
                ["置顶", found.pinned, commands.length],
                ["全部", found.others, commands.length + found.pinned.length],
              ] as const
            ).map(
              ([title, list, offset]) =>
                list.length > 0 && (
                  <div key={title}>
                    <p className="px-2 pt-1 pb-0.5 text-xs text-muted-foreground">{title}</p>
                    {list.map((t, i) => (
                      <button
                        key={t.id}
                        type="button"
                        role="option"
                        aria-selected={offset + i === active}
                        data-index={offset + i}
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => insert(t)}
                        onMouseMove={() => setActive(offset + i)}
                        className={`flex h-7 w-full items-center gap-2 rounded-control px-2 text-left ${offset + i === active ? "bg-accent" : ""}`}
                      >
                        <span className="min-w-0 flex-1 truncate">{t.title}</span>
                        {t.category && <span className="shrink-0 text-xs text-muted-foreground">{t.category}</span>}
                      </button>
                    ))}
                  </div>
                ),
            )}
        </div>
      )}
      <div className={`rounded-card border bg-card focus-within:ring-2 focus-within:ring-ring ${props.dropping ? "ring-2 ring-primary/60" : ""}`}>
        {props.chips}
        <div className="relative px-4 pt-2">
          {marks.length > 0 && (
            <div
              ref={mirror}
              aria-hidden
              className="pointer-events-none absolute inset-x-4 top-2 bottom-0 overflow-hidden py-1 font-reading text-[15px] leading-relaxed break-words whitespace-pre-wrap text-transparent"
            >
              {marks.map((m, i) => {
                const prev = i === 0 ? 0 : marks[i - 1].end;
                const current = sel[0] < m.end && sel[1] > m.start ? true : sel[0] === sel[1] && sel[0] >= m.start && sel[0] <= m.end;
                return (
                  <span key={m.start}>
                    {text.slice(prev, m.start)}
                    <mark className={`rounded-sm text-transparent ${current ? "bg-primary/30" : "bg-primary/15"}`}>{text.slice(m.start, m.end)}</mark>
                  </span>
                );
              })}
              {text.slice(marks[marks.length - 1].end)}
              {"​"}
            </div>
          )}
          <textarea
            ref={area}
            rows={1}
            value={text}
            spellCheck={false}
            aria-label="问题"
            placeholder="想问点什么……"
            onChange={(e) => change(e.target.value)}
            onSelect={(e) => setSel([e.currentTarget.selectionStart, e.currentTarget.selectionEnd])}
            onScroll={(e) => mirror.current && (mirror.current.scrollTop = e.currentTarget.scrollTop)}
            onKeyDown={onKeyDown}
            className="relative block min-h-7 w-full resize-none bg-transparent py-1 font-reading text-[15px] leading-relaxed outline-none placeholder:text-muted-foreground"
          />
        </div>
        <div className="flex items-center gap-0.5 px-2 pt-1 pb-2">
          {props.before}
          <Dropdown<number | null>
            plain
            ariaLabel="模板"
            value={null}
            options={menuOptions}
            actions={[{ label: "管理模板…", onSelect: props.onManageTemplates }]}
            onChange={(id) => {
              const t = props.templates.find((x) => x.id === id);
              if (t) insert(t);
            }}
            menuMaxHeight={320}
            renderTrigger={() => (
              <span className="flex items-center gap-1">
                <FileText size={14} strokeWidth={1.5} /> 模板
              </span>
            )}
            triggerClassName={toolClass}
          />
          {props.tools}
          <div className="flex-1" />
          {props.busy ? (
            <button type="button" aria-label="停止" title="停止" onClick={props.onStop} className="flex size-8 shrink-0 items-center justify-center rounded-full bg-foreground text-background hover:opacity-80">
              <Square size={12} fill="currentColor" />
            </button>
          ) : (
            <button
              type="button"
              aria-label="发送"
              title="发送"
              disabled={!text.trim() && !props.allowEmpty}
              onClick={() => void send()}
              className="flex size-8 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground hover:opacity-90 disabled:opacity-30"
            >
              <ArrowUp size={16} strokeWidth={2} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
});

export { toolClass };
