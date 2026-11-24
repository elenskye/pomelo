// 见字如面's writing box: one article's Markdown on LiveText with the Bear flavor (bear.ts), saved as it's written
// (a pause of 0.6 s, leaving, switching), one ⌘Z step per save in the app (the box keeps its own ⌘Z while typing).
// - A new article is created by its first save (no empty ones are kept): it starts as its seed (an empty heading, the
//   tag of the list it's written in), the caret on the first line, and is kept once something is written besides.
// - Format bar: always there (floating over the bottom edge of the text) or only over a selection; the pin switches.
//   Narrow: 引用 / 代码 / 柚子链接 move into 「更多格式」 (its 提示框 opens a second strip, as the right-click list does). Right-click (in layers, bear.ts `formats`): 记到念念不忘 and
//   柚子链接… (LinkPicker), 文字 / 段落 / 插入 / 提示框 (and 表格 in a table), 柚子's 润色 / 续写, then the two icon libraries.
// - Keys (Bear's; 念念不忘's where they differ; ⌘1–9 are the 页签): ⌘B ⌘I ⌘U ⌘⇧E ⌘⇧H ⌘⇧C, ⌘⇧K link, ⌘⇧L task,
//   ⌥⌘1–6 heading, ⌥⌘0 plain; in a table Tab / ⇧Tab move between cells and ↩ adds a row.
// - Pictures and files: pasted or dropped, stored as attachments (attachments.rs) and written in by name; the save
//   sends the ids the text uses. `[[` lists what to link to (柚子链接: articles, notes, books, films, games), `#` tags.
// - Clicks: a checkbox ticks; a fold arrow folds; a picture / formula / diagram puts the caret into its source (marks
//   show only where the caret is: bear.ts);
//   a link, [[link]] or #tag opens (⌘-click on the line being written); a set-in note's ↗ opens it in 念念不忘.
// - 柚子 (useAssistOn): 润色 a selection / 续写 at the caret → a card with the proposal, applied only on 「用这段」.
import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef, useState, type CSSProperties, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useQueryClient } from "@tanstack/react-query";
import { attachmentsApi, fileSrc, type FileInfo } from "@/shared/attachments/api";
import { pickAttachments } from "@/shared/attachments/Attachments";
import { useAttachTarget } from "@/shared/attachments/target";
import { offsetOf } from "@/shared/editor/editable";
import { LiveText, type Format, type LiveTextHandle, type Suggest } from "@/shared/editor/LiveText";
import { useSideStrip } from "@/shared/editor/MdMenu";
import { errorText, useToast } from "@/shared/feedback";
import { api } from "@/shared/ipc";
import {
  Bold,
  Brackets,
  ChevronDown,
  ChevronRight,
  Code,
  Ellipsis,
  Feather,
  Flame,
  Heading,
  Highlighter,
  Italic,
  Lightbulb,
  Link2,
  List,
  ListChecks,
  ListOrdered,
  Paperclip,
  PenLine,
  Pin,
  PinOff,
  Quote,
  Sparkles,
  Star,
  Strikethrough,
  TriangleAlert,
  Underline,
} from "@/shared/icons";
import { Tip } from "@/shared/handParts";
import { Button } from "@/shared/ui";
import { DiffView, assistBusyText, useAssistOn } from "@/modules/youzi/Assist";
import { useYouziStatus } from "@/modules/youzi/api";
import { ARTICLES_KEY, articlesApi, type Article, type Link } from "./api";
import { foundIn, linksChanged, useBacklinks, useLookup } from "@/shared/links/links";
import { BacklinkChips, linkInsert, typingLink, useLinkSource, useOpenThing, useStickerHtml } from "@/shared/links/LinkParts";
import { escapeName } from "@/shared/links/links";
import { ClipBox } from "@/shared/links/ClipBox";
import { LinkPicker, linkText } from "@/shared/links/LinkPicker";
import { selectionAnchor, useDragFit, type Anchor } from "@/shared/fit";
import { CALLOUTS, HEADING, HIGHLIGHTS, LIST, bearFlavor, blocks, cells, decodeName, encodeName, foldKey, foldRange, markedFolds, type Look } from "./bear";
import { renderCode, renderDiagram, renderMath } from "./render";
import "./articles.css";

export type BarMode = "always" | "select";
/** The text's look (设置): font size (px), line height (× the size), the space after each line (em; a blank line is a whole line, as in Bear), the column's width (null: all of it), the bar. */
export type Look2 = { size: number; leading: number; para: number; width: number | null; bar: BarMode };

export type EditorHandle = {
  flush: () => Promise<void>;
  focus: () => void;
  /** 书架: something dragged over the text at (x, y) — shows where it would go (a line between paragraphs); false: not over the text. */
  dropHint: (x: number, y: number) => boolean;
  /** Puts `line` there as a paragraph of its own (one ⌘Z step); false: not over the text. */
  dropLine: (x: number, y: number, line: string) => boolean;
};

const isDark = () => document.documentElement.dataset.theme === "dark";

// ---------- text helpers ----------

const lineStarts = (text: string) => {
  const out = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === "\n") out.push(i + 1);
  return out;
};
const lineAt = (text: string, pos: number) => text.slice(0, pos).split("\n").length - 1;

/** Names written into the text (pictures and file links), decoded. */
function namesIn(text: string): Set<string> {
  const out = new Set<string>();
  for (const m of text.matchAll(/\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g)) out.add(decodeName(m[1]));
  return out;
}

const pad2 = (n: number) => String(n).padStart(2, "0");
/** A pasted file under a name the article doesn't use yet (a screenshot: 截图 2026-11-13 10.32.05.png). */
function named(f: File, i: number, taken: Map<string, FileInfo>): File {
  const d = new Date();
  const ext = (/\.[a-z0-9]+$/i.exec(f.name)?.[0] ?? (f.type === "image/jpeg" ? ".jpg" : ".png")).toLowerCase();
  const pasted = !f.name || /^image\.(png|jpe?g|gif|tiff?)$/i.test(f.name);
  const base = pasted ? `截图 ${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}.${pad2(d.getMinutes())}.${pad2(d.getSeconds())}${i ? ` ${i + 1}` : ""}` : f.name.replace(/\.[^.]+$/, "");
  let name = `${base}${pasted ? ext : (/\.[^.]+$/.exec(f.name)?.[0] ?? "")}`;
  for (let n = 2; taken.has(name); n++) name = `${base} ${n}${pasted ? ext : (/\.[^.]+$/.exec(f.name)?.[0] ?? "")}`;
  return name === f.name ? f : new File([f], name, { type: f.type });
}

// ---------- the bar ----------

function BarButton(props: { label: string; onClick: (e: ReactMouseEvent<HTMLElement>) => void; children: ReactNode; on?: boolean; keys?: string }) {
  return (
    <button
      type="button"
      aria-label={props.label}
      aria-pressed={props.on}
      onMouseDown={(e) => e.preventDefault()}
      onClick={props.onClick}
      className={`ar-fb pm-tip-host ${props.on ? "on" : ""}`}
    >
      {props.children}
      <Tip>{props.keys ? `${props.label} ${props.keys}` : props.label}</Tip>
    </button>
  );
}

const CALLOUT_ICONS: Record<string, typeof Flame> = { "pen-line": PenLine, lightbulb: Lightbulb, star: Star, "triangle-alert": TriangleAlert, flame: Flame };
/** A callout's icon (bear.ts CALLOUTS) for a menu row. */
function CalloutIcon(props: { name: string }) {
  const Icon = CALLOUT_ICONS[props.name];
  return Icon ? <Icon size={16} /> : null;
}

type MenuRow = { label: string; keys?: string; run: () => void; swatch?: string | null; icon?: ReactNode };
/** A row, a line, or rows folded into one (提示框) that opens a second strip beside it, as the right-click list does. */
type MenuItem = MenuRow | { group: string; items: MenuRow[] } | "-";
function PopMenu(props: { at: { left: number; top: number; up: boolean }; items: MenuItem[]; onClose: () => void; label: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const strip = useRef<HTMLDivElement>(null);
  const side = useSideStrip(strip, props.at);
  useEffect(() => {
    const away = (e: PointerEvent) => !ref.current?.contains(e.target as Node) && props.onClose();
    const key = (e: KeyboardEvent) => e.key === "Escape" && (e.stopPropagation(), side.sub ? side.open(null) : props.onClose());
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("keydown", key, true);
    };
  });
  // the mark (swatch, icon) sits by its words; the keys go to the far end
  const row = (it: MenuRow, i: number) => (
    <button
      key={i}
      type="button"
      role="menuitem"
      className="pm-menu-item md-menu-item"
      onMouseEnter={(e) => e.currentTarget.closest(".md-sub") || side.rest(null)}
      onClick={() => {
        props.onClose();
        it.run();
      }}
    >
      <span className="flex min-w-0 items-center gap-2">
        {it.swatch !== undefined && <span className={`ar-swatch hl-${it.swatch ?? "def"}`} aria-hidden />}
        {it.icon}
        <span className="truncate">{it.label}</span>
      </span>
      {it.keys && <span className="md-menu-keys">{it.keys}</span>}
    </button>
  );
  const group = side.sub ? props.items.find((it): it is { group: string; items: MenuRow[] } => typeof it === "object" && "group" in it && it.group === side.sub!.name) : undefined;
  return createPortal(
    <div ref={ref} onMouseDown={(e) => e.preventDefault()}>
      <div ref={strip} role="menu" aria-label={props.label} className="pm-menu ar-pop animate-pop-in" style={{ left: props.at.left, ...(props.at.up ? { bottom: window.innerHeight - props.at.top } : { top: props.at.top }) }}>
        {props.items.map((it, i) =>
          it === "-" ? (
            <div key={i} className="md-menu-rule" aria-hidden />
          ) : "group" in it ? (
            <button
              key={i}
              type="button"
              role="menuitem"
              aria-haspopup="true"
              aria-expanded={side.sub?.name === it.group}
              className={`pm-menu-item md-menu-item ${side.sub?.name === it.group ? "on" : ""}`}
              onMouseEnter={(e) => side.rest({ name: it.group, row: e.currentTarget })}
              onClick={(e) => side.open({ name: it.group, row: e.currentTarget })}
            >
              <span>{it.group}</span>
              <ChevronRight size={14} className="md-menu-keys" />
            </button>
          ) : (
            row(it, i)
          ),
        )}
      </div>
      {group && (
        <div ref={side.side} role="menu" aria-label={group.group} className="pm-menu ar-pop md-sub" style={{ left: side.at?.left ?? 0, top: side.at?.top ?? 0, visibility: side.at ? "visible" : "hidden" }} onMouseEnter={side.hold}>
          {group.items.map(row)}
        </div>
      )}
    </div>,
    document.body,
  );
}

// ---------- 柚子's card ----------

type AssistState = { mode: "polish" | "continue"; from: [number, number]; text: string; after: string; result?: string; error?: string };

function AssistCard(props: { state: AssistState; at: Anchor; onRun: () => void; onUse: (text: string) => void; onClose: () => void }) {
  const status = useYouziStatus().data;
  const s = props.state;
  const working = s.result === undefined && !s.error;
  const box = useRef<HTMLDivElement>(null);
  const { style: place, grab } = useDragFit(box, props.at);
  const name = s.mode === "polish" ? "柚子润色" : "柚子续写";
  return createPortal(
    // a hand-drawn card like the panels (.pm-float), kept in the window as the reply grows; the reply scrolls inside;
    // dragged by its title or empty parts to see the text under it
    <div ref={box} role="dialog" aria-label={name} className="ar-assist pm-float animate-pop-in" style={place} data-hand="card" data-seed={1547} data-amp="0.8" data-drag="" onMouseDown={grab}>
      <p className="flex shrink-0 items-center gap-1.5 font-bold">
        <Sparkles size={15} className="text-[var(--module-youzi)]" />
        {name}
      </p>
      {(working || s.error || s.result !== undefined) && (
        <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
          {working && <p className="animate-pulse text-sm text-muted-foreground">{assistBusyText(status, s.mode === "polish" ? "正在润色…" : "正在续写…")}</p>}
          {s.error && <p className="text-sm text-destructive">{s.error}</p>}
          {s.result !== undefined && (s.mode === "polish" ? <DiffView from={s.text} to={s.result} /> : <p className="selectable leading-relaxed whitespace-pre-wrap">{s.result}</p>)}
        </div>
      )}
      <div className="flex shrink-0 items-center justify-end gap-2">
        {!working && <Button variant="ghost" onClick={props.onRun}>再来一次</Button>}
        <Button variant="ghost" onClick={props.onClose}>取消</Button>
        {s.result !== undefined && <Button variant="primary" onClick={() => props.onUse(s.result!)}>用这段</Button>}
      </div>
    </div>,
    document.body,
  );
}

// ---------- the editor ----------

export const ArticleEditor = forwardRef<
  EditorHandle,
  {
    /** null: a new one, created by its first save. */
    article: Article | null;
    /** A new one's first text (an empty heading; in a tag view the tag on the second line); the caret ends line 1. */
    seed?: string;
    titles: Link[];
    tags: string[];
    look: Look2;
    onBar: (mode: BarMode) => void;
    /** The article now has an id (a new one got saved). */
    onCreated: (a: Article) => void;
    onOpenTitle: (title: string) => void;
    onOpenTag: (tag: string) => void;
    onOpenId: (id: number) => void;
    readOnly?: boolean;
    /** Header row over the text (the article's ⋯, 专心写). */
    head?: ReactNode;
  }
>(function ArticleEditor(props, handle) {
  const qc = useQueryClient();
  const toast = useToast();
  const assistOn = useAssistOn();
  const live = useRef<LiveTextHandle>(null);
  const box = useRef<HTMLDivElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const initial = props.article?.content ?? props.seed ?? "";
  const [text, setText] = useState(initial);
  const textRef = useRef(initial);
  const saved = useRef(props.article?.content ?? "");
  const id = useRef<number | null>(props.article?.id ?? null);
  const creating = useRef<Promise<void> | null>(null);
  const timer = useRef<number | null>(null);
  const [files, setFiles] = useState<Map<string, FileInfo>>(() => new Map((props.article?.attachments ?? []).map((f) => [f.name, f])));
  const filesRef = useRef(files);
  filesRef.current = files;
  const [folds, setFolds] = useState<Set<string>>(() => markedFolds(initial));
  const foldsRef = useRef(folds);
  foldsRef.current = folds;
  const titlesRef = useRef(props.titles);
  titlesRef.current = props.titles;
  /** 0 wide, 1 引用 / 代码 / 互链 into 「更多格式」, 2 the lists too. */
  const [narrow, setNarrow] = useState(0);
  const [menu, setMenu] = useState<{ kind: "heading" | "highlight" | "more"; at: { left: number; top: number; up: boolean } } | null>(null);
  const [floatAt, setFloatAt] = useState<{ left: number; top: number } | null>(null);
  const [assist, setAssist] = useState<(AssistState & { at: Anchor }) | null>(null);
  /** 记到念念不忘's box, 链接到…'s finder (over the words picked then). */
  const [clip, setClip] = useState<{ at: Anchor; text: string } | null>(null);
  const [linking, setLinking] = useState<{ at: Anchor; sel: [number, number] } | null>(null);

  // ---------- saving ----------
  // one save at a time, in the order asked: the commands run off the main thread (articles.rs), so two sent at once
  // could land the other way round. A save waiting its turn takes the text as it is by then.
  const saving = useRef<Promise<void>>(Promise.resolve());
  const save = () => {
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = null;
    live.current?.flush();
    return (saving.current = saving.current.then(saveNow, saveNow));
  };
  const saveNow = async () => {
    const content = textRef.current;
    if (creating.current) await creating.current;
    if (content === saved.current) return;
    const used = namesIn(content);
    const attachments = [...filesRef.current.values()].filter((f) => used.has(f.name)).map((f) => f.id);
    try {
      if (id.current === null) {
        if (content === (props.seed ?? "") || !content.replace(/^#{1,6}[ \t]*$/gm, "").trim()) return;
        const job = articlesApi.create({ content, attachments }).then((a) => {
          id.current = a.id;
          saved.current = content;
          props.onCreated(a);
        });
        creating.current = job;
        await job;
        creating.current = null;
      } else {
        await articlesApi.update({ id: id.current, patch: { content, attachments } });
        saved.current = content;
      }
      void qc.invalidateQueries({ queryKey: ARTICLES_KEY, predicate: (q) => q.queryKey[1] !== "one" });
      linksChanged(qc);
    } catch (e) {
      creating.current = null;
      toast(errorText(e), "error");
    }
  };
  const saveRef = useRef(save);
  saveRef.current = save;
  // ---------- 书架: a note dropped between paragraphs ----------
  const [dropAt, setDropAt] = useState<{ top: number; left: number; width: number } | null>(null);
  /** The gap nearest (x, y): after line `after` (-1: before the first), and where to draw it in the scroller. */
  const gapAt = (x: number, y: number) => {
    const root = box.current;
    const sc = scroller.current;
    if (!root || !sc) return null;
    const r = sc.getBoundingClientRect();
    if (x < r.left || x > r.right || y < r.top || y > r.bottom) return null;
    const rows = Array.from(root.children) as HTMLElement[];
    let after = rows.length - 1;
    for (let i = 0; i < rows.length; i++) {
      const b = rows[i].getBoundingClientRect();
      if (!b.height) continue;
      if (y < b.top + b.height / 2) {
        after = i - 1;
        break;
      }
    }
    // not inside a block (a table, code, a quote): after its last line
    const lines = current().split("\n");
    const blk = blocks(lines).find((bk) => after >= bk.from && after < bk.to);
    if (blk) after = blk.to;
    const ref = rows[Math.max(0, after)]?.getBoundingClientRect();
    const rb = root.getBoundingClientRect();
    if (!ref) return { after, top: 0, left: 0, width: rb.width };
    const top = (after < 0 ? ref.top : ref.bottom) - r.top + sc.scrollTop;
    return { after, top, left: rb.left - r.left, width: rb.width };
  };
  useImperativeHandle(handle, () => ({
    flush: () => saveRef.current(),
    focus: () => live.current?.focus(),
    dropHint: (x, y) => {
      const g = gapAt(x, y);
      setDropAt(g ? { top: g.top, left: g.left, width: g.width } : null);
      return !!g;
    },
    dropLine: (x, y, line) => {
      const g = gapAt(x, y);
      setDropAt(null);
      if (!g || props.readOnly) return false;
      const t = current();
      const lines = t.split("\n");
      const starts = lineStarts(t);
      if (g.after < 0) edit(0, 0, `${line}\n\n`, [line.length, line.length]);
      else {
        const end = starts[g.after] + lines[g.after].length;
        edit(end, end, `\n\n${line}`, [end + 2 + line.length, end + 2 + line.length]);
      }
      return true;
    },
  }));
  useEffect(() => () => void saveRef.current(), []);
  // a new one: the caret at the end of its first line (the heading)
  useEffect(() => {
    if (props.article || props.readOnly || props.seed === undefined) return;
    const end = props.seed.indexOf("\n");
    requestAnimationFrame(() => live.current?.select(end < 0 ? props.seed!.length : end));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const onChange = (v: string) => {
    textRef.current = v;
    setText(v);
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void saveRef.current(), 600);
  };
  // changed elsewhere (柚子's card, a tag renamed) while nothing here is unsaved: take it
  useEffect(() => {
    const a = props.article;
    if (!a || a.id !== id.current) return;
    setFiles(new Map(a.attachments.map((f) => [f.name, f])));
    if (a.content !== saved.current && textRef.current === saved.current) {
      saved.current = a.content;
      textRef.current = a.content;
      setText(a.content);
    }
  }, [props.article?.updatedAt, props.article?.content]); // eslint-disable-line react-hooks/exhaustive-deps

  // ---------- 柚子链接: what the text's links name, their stickers ----------
  const things = useLookup(useMemo(() => (text.includes("[[") ? foundIn(text).map((x) => x.target) : []), [text]));
  const thingsRef = useRef(things);
  thingsRef.current = things;
  const thingsKey = [...things].map(([k, v]) => `${k}=${v ? `${v.module}${v.id}:${v.title}` : v}`).join("|");
  const sticker = useStickerHtml(16);
  const stickerRef = useRef(sticker);
  stickerRef.current = sticker;
  const openThing = useOpenThing();

  // ---------- look ----------
  const look = useRef<() => Look>(() => ({} as Look));
  look.current = () => ({
    image: (name) => {
      const f = filesRef.current.get(name);
      return f && f.kind === "image" && !f.missing ? fileSrc(f.id) : null;
    },
    file: (name) => filesRef.current.has(name),
    thing: (t) => thingsRef.current.get(t),
    sticker: (m) => stickerRef.current(m),
    math: renderMath,
    mermaid: (src) => renderDiagram(src, isDark(), () => live.current?.redraw()),
    code: renderCode,
    folds: foldsRef.current,
  });
  // table rows / columns and 柚子 in the right-click list, by where the caret is
  const extra = useRef<() => Format[]>(() => []);
  const flavor = useMemo(() => bearFlavor(() => look.current(), () => extra.current()), []);
  useEffect(() => live.current?.redraw(), [files, folds, props.titles, thingsKey, sticker]);

  // ---------- edits on the text ----------
  const edit = (from: number, to: number, insert: string, sel?: [number, number]) => {
    const at = from + insert.length;
    live.current?.edit(from, to, insert, sel ?? [at, at]);
  };
  const current = () => live.current?.text() ?? textRef.current;
  const caretLine = () => {
    const t = current();
    const [s] = live.current?.selection() ?? [0, 0];
    return { t, s, line: lineAt(t, s), starts: lineStarts(t) };
  };
  const tableAt = () => {
    const { t, s, line, starts } = caretLine();
    const lines = t.split("\n");
    const b = blocks(lines).find((x) => x.kind === "table" && line >= x.from && line <= x.to);
    return b ? { t, s, line, starts, lines, b } : null;
  };
  const tableOps = (): Format[] => {
    const tb = tableAt();
    if (!tb) return [];
    const { lines, b, line, starts, s } = tb;
    const col = Math.max(0, cells(lines[line]).findIndex(([a, e]) => s - starts[line] >= a && s - starts[line] <= e));
    const rewrite = (next: string[], caretRow: number) => {
      const from = starts[b.from];
      const to = starts[b.to] + lines[b.to].length;
      const body = next.join("\n");
      const ls = lineStarts(body);
      const at = from + (ls[Math.min(caretRow, ls.length - 1)] ?? 0) + 2;
      edit(from, to, body, [at, at]);
    };
    const rows = lines.slice(b.from, b.to + 1);
    const n = Math.max(...rows.map((r) => cells(r).length));
    const blank = `|${"  |".repeat(n)}`;
    const ops: Format[] = [
      { label: "下面加一行", act: () => rewrite([...rows.slice(0, Math.max(2, line - b.from + 1)), blank, ...rows.slice(Math.max(2, line - b.from + 1))], Math.max(2, line - b.from + 1)) },
      { label: "右边加一列", act: () => rewrite(rows.map((r, k) => { const c = cells(r); const at = c[col] ? c[col][1] : r.length; return r.slice(0, at) + (k === 1 ? "| --- " : "|  ") + r.slice(at); }), line - b.from) },
    ];
    if (line - b.from >= 2 && rows.length > 3) ops.push({ label: "删掉这一行", act: () => rewrite(rows.filter((_, k) => k !== line - b.from), Math.min(line - b.from, rows.length - 2)) });
    if (n > 1) ops.push({ label: "删掉这一列", act: () => rewrite(rows.map((r) => { const c = cells(r); return c[col] ? r.slice(0, c[col][0] - 1) + r.slice(c[col][1]) : r; }), line - b.from) });
    return ops.map((f) => (f === "-" ? f : { ...f, group: "表格" }));
  };
  const askYouzi = (mode: "polish" | "continue", sel: [number, number]) => {
    const t = current();
    const [s, e] = sel;
    const text = mode === "polish" ? t.slice(s, e) : t.slice(0, e);
    const after = mode === "continue" ? t.slice(e) : "";
    const at = selectionAnchor();
    const state: AssistState = { mode, from: [s, e], text, after };
    setAssist({ ...state, at });
    runAssist(state, at);
  };
  const runAssist = (state: AssistState, at: Anchor) => {
    setAssist({ ...state, result: undefined, error: undefined, at });
    const go = async () => {
      if (id.current === null) await saveRef.current();
      if (id.current === null) throw new Error("先写点什么");
      return articlesApi.assist({ id: id.current, mode: state.mode, text: state.text, after: state.after || null });
    };
    go().then(
      (r) => setAssist((x) => (x && x.from === state.from ? { ...x, result: r.text } : x)),
      (e) => setAssist((x) => (x && x.from === state.from ? { ...x, error: errorText(e) } : x)),
    );
  };
  // 记到念念不忘: the picked words and 「来自 [[这篇]]」 into 念念不忘's box (saved first, for the title the link names)
  const clipNote = async (sel: [number, number]) => {
    const words = current().slice(sel[0], sel[1]).trim();
    if (!words) return toast("先选一段");
    const at = selectionAnchor();
    await saveRef.current();
    const title = id.current !== null ? (await articlesApi.get(id.current).catch(() => null))?.title : null;
    setClip({ at, text: title ? `${words}\n来自 [[${escapeName(title)}]]\n` : `${words}\n` });
  };
  // 柚子链接…: the picked words become a Pomelo link; nothing picked, a chip at the caret
  const linkTo = (t: { target: string }, sel: [number, number]) => {
    const text = linkText(t.target, current().slice(sel[0], sel[1]));
    edit(sel[0], sel[1], text);
    setLinking(null);
    live.current?.select(sel[0] + text.length); // back in the text, the caret after the link
  };
  // the right-click list's own rows (bear.ts `formats` puts them among Bear's): 记到念念不忘 and 柚子链接… on top, 公式 /
  // 流程图 / 脚注 into 插入, the five callouts, a table's rows and columns, 柚子
  extra.current = () => {
    const out: Format[] = [
      { label: "公式", group: "插入", act: () => insertBlock("$\nE = mc^2\n$", 3) },
      { label: "流程图", group: "插入", act: () => insertBlock("```mermaid\nflowchart LR\n  开始 --> 结束\n```", 11) },
      { label: "脚注", group: "插入", act: footnote },
      ...Object.entries(CALLOUTS).map(([k, c]): Format => ({ label: c.label, group: "提示框", icon: <CalloutIcon name={c.icon} />, act: () => insertBlock(`> [!${k.toUpperCase()}]\n> `, `> [!${k.toUpperCase()}]\n> `.length) })),
      ...tableOps(),
    ];
    if (!props.readOnly) {
      out.push({ label: "记到念念不忘", lead: true, icon: <Feather size={16} />, act: (sel) => void clipNote(sel) });
      out.push({ label: "柚子链接…", lead: true, icon: <Brackets size={16} />, act: (sel) => setLinking({ at: selectionAnchor(), sel }) });
    }
    if (assistOn && !props.readOnly && !props.article?.locked) {
      out.push("-", { label: "柚子润色", act: (sel) => (sel[0] === sel[1] ? toast("先选一段") : askYouzi("polish", sel)) }, { label: "柚子续写", act: (sel) => askYouzi("continue", [sel[1], sel[1]]) });
    }
    return out;
  };

  const setHeading = (level: number) => {
    const { t, line, starts } = caretLine();
    const l = t.split("\n")[line];
    const rest = l.replace(/^#{1,6}[ \t]+/, "");
    const next = level ? `${"#".repeat(level)} ${rest}` : rest;
    const at = starts[line] + next.length;
    edit(starts[line], starts[line] + l.length, next, [at, at]);
  };
  /** A block on a line of its own (the caret's line when empty, else the next), the caret `caretIn` into it. */
  const insertBlock = (block: string, caretIn = 0) => {
    const t = current();
    const [, e] = live.current?.selection() ?? [t.length, t.length];
    const ls = t.lastIndexOf("\n", e - 1) + 1;
    const le = t.indexOf("\n", e) < 0 ? t.length : t.indexOf("\n", e);
    if (!t.slice(ls, le).trim()) return edit(ls, le, block, [ls + caretIn, ls + caretIn]);
    edit(le, le, `\n${block}`, [le + 1 + caretIn, le + 1 + caretIn]);
  };
  const footnote = () => {
    const t = current();
    const used = [...t.matchAll(/\[\^(\d+)\]/g)].map((m) => Number(m[1]));
    const n = (used.length ? Math.max(...used) : 0) + 1;
    const [, e] = live.current?.selection() ?? [t.length, t.length];
    const tail = `${t.endsWith("\n") || !t ? "" : "\n"}\n[^${n}]: `;
    const next = t.slice(0, e) + `[^${n}]` + t.slice(e) + tail;
    live.current?.edit(0, t.length, next, [next.length, next.length]);
  };
  const foldAll = (shut: boolean) => {
    const lines = current().split("\n");
    const fenced = new Set<number>(blocks(lines).filter((b) => b.kind === "fence" || b.kind === "math").flatMap((b) => Array.from({ length: b.to - b.from + 1 }, (_, k) => b.from + k)));
    const keys = lines.map((_, i) => (HEADING.test(lines[i]) && foldRange(lines, i, fenced) ? foldKey(lines, i) : null)).filter((k): k is string => k !== null);
    setFolds(shut ? new Set(keys) : new Set());
  };
  const addFiles = async (take: Promise<FileInfo[]>) => {
    try {
      const got = await take;
      const map = new Map(filesRef.current);
      const lines: string[] = [];
      for (const f of got) {
        map.set(f.name, f);
        lines.push(f.kind === "image" ? `![](${encodeName(f.name)})` : `[${f.name}](${encodeName(f.name)})`);
      }
      setFiles(map);
      filesRef.current = map;
      live.current?.insertLines(lines);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const dropping = useAttachTarget(wrap, (fs) => void addFiles(Promise.resolve(fs)), (e) => toast(errorText(e), "error"), !props.readOnly);

  const F = {
    bold: { label: "加粗", keys: "⌘B", wrap: "**" },
    italic: { label: "斜体", keys: "⌘I", wrap: "*" },
    underline: { label: "下划线", keys: "⌘U", wrap: "~" },
    strike: { label: "删除线", keys: "⌘⇧E", wrap: "~~" },
    code: { label: "行内代码", keys: "⌘⇧C", wrap: "`" },
    link: { label: "链接", keys: "⌘⇧K", around: ["[", "](https://)"] as [string, string] },
    wiki: { label: "柚子链接", keys: "[[", around: ["[[", "]]"] as [string, string] },
    list: { label: "列表", line: "- " },
    ordered: { label: "编号列表", line: "1. " },
    task: { label: "待办", keys: "⌘⇧L", line: "- [ ] " },
    quote: { label: "引用", line: "> " },
  } satisfies Record<string, Exclude<Format, "-">>;
  const apply = (f: Exclude<Format, "-">) => live.current?.applyFormat(f);

  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const k = e.key.toLowerCase();
    if (e.metaKey && e.altKey && /^Digit[0-6]$/.test(e.code)) {
      e.preventDefault();
      setHeading(Number(e.code.slice(5)));
      return true;
    }
    if (e.metaKey && e.shiftKey && !e.altKey && k === "k") {
      e.preventDefault();
      apply(F.link);
      return true;
    }
    if (e.metaKey && e.shiftKey && !e.altKey && k === "l") {
      e.preventDefault();
      apply(F.task);
      return true;
    }
    // ⌫ right after a heading's marks (a unit WebKit won't delete): the line is text again
    if (e.key === "Backspace" && !e.metaKey && !e.altKey && !e.ctrlKey && !e.shiftKey) {
      const [s, end] = live.current?.selection() ?? [0, 0];
      const { t, line, starts } = caretLine();
      const lines = t.split("\n");
      const h = s === end ? HEADING.exec(lines[line] ?? "") : null;
      const code = () => blocks(lines).some((b) => (b.kind === "fence" || b.kind === "math") && line >= b.from && line <= b.to);
      if (h && s - starts[line] === h[1].length + h[2].length && !code()) {
        e.preventDefault();
        edit(starts[line], s, "", [starts[line], starts[line]]);
        return true;
      }
    }
    const tb = (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey && !e.metaKey)) && tableAt();
    if (tb) {
      const { lines, line, starts, s, b } = tb;
      if (e.key === "Tab") {
        e.preventDefault();
        const all = Array.from({ length: b.to - b.from + 1 }, (_, k) => b.from + k)
          .filter((r) => r !== b.from + 1)
          .flatMap((r) => cells(lines[r]).map(([a, z]) => [starts[r] + a, starts[r] + z] as [number, number]));
        const i = all.findIndex(([a, z]) => s >= a && s <= z);
        const next = all[i + (e.shiftKey ? -1 : 1)];
        if (next) {
          const t = current().slice(next[0], next[1]);
          const lead = t.length - t.trimStart().length;
          live.current?.select(next[0] + Math.min(lead, 1), next[1] - (t.endsWith(" ") ? 1 : 0));
        }
        return true;
      }
      if (line !== b.from + 1) {
        e.preventDefault();
        const n = cells(lines[line]).length;
        const row = `|${"  |".repeat(n)}`;
        const end = starts[line] + lines[line].length;
        edit(end, end, `\n${row}`, [end + 3, end + 3]);
        return true;
      }
    }
    return false;
  };

  // ---------- clicks on what's drawn ----------
  const onMouseDown = (e: ReactMouseEvent) => {
    const el = e.target as HTMLElement;
    const root = box.current;
    if (!root || e.button !== 0) return;
    if (!root.contains(el)) {
      // the bare page below a short text: the caret goes to its end
      if ((el === scroller.current || el.classList.contains("ar-page")) && !props.readOnly) {
        e.preventDefault();
        live.current?.select(current().length);
      }
      return;
    }
    const ln = el.closest<HTMLElement>(".ln");
    const index = ln ? Array.from(root.children).indexOf(ln) : -1;
    const t = current();
    const starts = lineStarts(t);
    const active = !!ln?.classList.contains("cur");
    const stop = () => {
      e.preventDefault();
      e.stopPropagation();
    };
    if (el.closest("[data-task]") && index >= 0 && !props.readOnly) {
      stop();
      const l = t.split("\n")[index];
      const m = LIST.exec(l);
      if (m?.[4]) {
        const at = starts[index] + m[1].length + m[2].length + m[3].length + 1;
        const on = /x/i.test(m[4][1]);
        live.current?.edit(at, at + 1, on ? " " : "x", live.current.selection());
      }
      return;
    }
    // the arrow, or the 「…」 after a folded line (opens it, as in Bear)
    const fold = el.closest<HTMLElement>("[data-fold]") ?? (el.closest(".folded") ? ln?.querySelector<HTMLElement>(":scope > [data-fold]") : null);
    if (fold) {
      stop();
      const key = fold.dataset.fold!;
      const next = new Set(foldsRef.current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      setFolds(next);
      return;
    }
    const noteId = el.closest<HTMLElement>("[data-open-note]")?.dataset.openNote;
    if (noteId) {
      stop();
      openThing({ module: "notes", id: Number(noteId), title: "" });
      return;
    }
    // a link's sticker is drawn (data-pv) but belongs to the link: open it like the name
    const stickerLink = el.closest<HTMLElement>(".pm-lk-mk")?.closest<HTMLElement>("[data-link]");
    const pvEl = stickerLink ? null : el.closest<HTMLElement>("[data-pv]");
    if (pvEl && index >= 0) {
      stop();
      // an inline formula / picture: into its source (its group then shows it); a block: the end of its first line
      const g = pvEl.dataset.at === undefined ? pvEl.parentElement?.closest<HTMLElement>(".fx") : null;
      const at = pvEl.dataset.at !== undefined ? Number(pvEl.dataset.at) : index;
      const pos = g ? offsetOf(root, g, g.childNodes.length, starts) - 1 : starts[at] + (t.split("\n")[at]?.length ?? 0);
      live.current?.select(pos);
      return;
    }
    if (!(e.metaKey || !active)) return;
    const href = el.closest<HTMLElement>("[data-href]")?.dataset.href;
    const wiki = el.closest<HTMLElement>("[data-link]")?.dataset.link;
    const tag = el.closest<HTMLElement>("[data-tag]")?.dataset.tag;
    const file = el.closest<HTMLElement>("[data-file]")?.dataset.file;
    if (href) {
      stop();
      void api.openLink(/^www\./.test(href) ? `https://${href}` : href).catch((err) => toast(errorText(err), "error"));
    } else if (wiki) {
      stop();
      const hit = thingsRef.current.get(wiki.trim());
      if (hit) openThing(hit);
      else props.onOpenTitle(wiki.trim());
    } else if (tag) {
      stop();
      props.onOpenTag(tag);
    } else if (file) {
      stop();
      const f = filesRef.current.get(file);
      if (f) void attachmentsApi.open(f.id).catch((err) => toast(errorText(err), "error"));
    }
  };

  // ---------- suggestions: [[ what to link to (柚子链接) and #tags ----------
  const links = useLinkSource(props.article ? ["articles", props.article.id] : null);
  const suggest: Suggest = useMemo(
    () => ({
      label: "补全",
      at: (value, caret) => {
        const before = value.slice(0, caret);
        const w = typingLink(value, caret);
        if (w) return { start: w.start, query: `\u0001${w.query}` };
        const m = /(?:^|\s)#([^\s#]*)$/u.exec(before);
        if (!m || /^#{1,6}$/.test(before.slice(before.lastIndexOf("\n") + 1).trim())) return null;
        return { start: caret - m[1].length - 1, query: m[1] };
      },
      items: (q) => {
        if (q.startsWith("\u0001")) return links.items(q.slice(1));
        return props.tags.includes(q) ? [] : props.tags.filter((t) => t !== q && t.toLowerCase().includes(q.toLowerCase())).sort((a, b) => Number(!a.startsWith(q)) - Number(!b.startsWith(q))).slice(0, 8);
      },
      show: (item) => (links.isLink(item) ? links.show(item) : `#${item}`),
      insert: (item, after) => {
        const t = links.isLink(item) ? links.thing(item) : null;
        return t ? linkInsert(t, after) : `#${item}${after.startsWith(" ") ? "" : " "}`;
      },
    }),
    [props.tags, links.version], // eslint-disable-line react-hooks/exhaustive-deps
  );

  // ---------- the bar ----------
  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setNarrow(el.clientWidth < 520 ? 2 : el.clientWidth < 700 ? 1 : 0));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  // the floating bar follows a selection
  useEffect(() => {
    if (props.look.bar !== "select" || props.readOnly) return setFloatAt(null);
    const on = () => {
      const sel = window.getSelection();
      const root = box.current;
      if (!sel || !root || sel.isCollapsed || !sel.rangeCount || !root.contains(sel.anchorNode)) return setFloatAt(null);
      const r = sel.getRangeAt(0).getBoundingClientRect();
      const w = wrap.current!.getBoundingClientRect();
      setFloatAt({ left: Math.max(w.left + 8, Math.min(r.left + r.width / 2 - 170, w.right - 348)), top: Math.max(w.top + 4, r.top - 50) });
    };
    document.addEventListener("selectionchange", on);
    return () => document.removeEventListener("selectionchange", on);
  }, [props.look.bar, props.readOnly]);

  const openMenu = (kind: "heading" | "highlight" | "more", e: ReactMouseEvent<HTMLElement>) => {
    const b = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const up = b.top > window.innerHeight / 2;
    setMenu({ kind, at: { left: Math.min(b.left, window.innerWidth - 220), top: up ? b.top - 6 : b.bottom + 6, up } });
  };
  const menuItems = (kind: "heading" | "highlight" | "more"): MenuItem[] => {
    if (kind === "heading") return [...[1, 2, 3, 4, 5, 6].map((n) => ({ label: `标题 ${n}`, keys: `⌥⌘${n}`, run: () => setHeading(n) })), "-", { label: "正文", keys: "⌥⌘0", run: () => setHeading(0) }];
    if (kind === "highlight") return HIGHLIGHTS.map((h) => ({ label: h.label, swatch: h.tint, keys: h.patch ? undefined : "⌘⇧H", run: () => apply(h.patch ? { label: "高亮", around: [`==${h.patch}`, "=="] } : { label: "高亮", wrap: "==" }) }));
    const lists: MenuItem[] = narrow > 1 ? [{ label: "列表", run: () => apply(F.list) }, { label: "编号列表", run: () => apply(F.ordered) }, { label: "待办", keys: "⌘⇧L", run: () => apply(F.task) }] : [];
    const optional: MenuItem[] = narrow ? [...lists, { label: "引用", run: () => apply(F.quote) }, { label: "行内代码", run: () => apply(F.code) }, { label: "柚子链接", keys: "[[", run: () => apply(F.wiki) }, "-"] : [];
    return [
      ...optional,
      { label: "代码块", run: () => apply({ label: "代码块", around: ["```\n", "\n```"], block: true }) },
      { label: "表格", run: () => insertBlock("| 列 1 | 列 2 |\n| --- | --- |\n|  |  |", 2) },
      { group: "提示框", items: Object.entries(CALLOUTS).map(([k, c]) => ({ label: c.label, icon: <CalloutIcon name={c.icon} />, run: () => insertBlock(`> [!${k.toUpperCase()}]\n> `, `> [!${k.toUpperCase()}]\n> `.length) })) },
      { label: "分隔线", run: () => insertBlock("---\n", 4) },
      { label: "公式", run: () => insertBlock("$$\nE = mc^2\n$$", 3) },
      { label: "流程图", run: () => insertBlock("```mermaid\nflowchart LR\n  开始 --> 结束\n```", 11) },
      { label: "脚注", run: footnote },
      "-",
      { label: "全部折起来", run: () => foldAll(true) },
      { label: "全部展开", run: () => foldAll(false) },
    ];
  };

  const bar = (floating: boolean) => (
    <div className={`ar-bar ${floating ? "float" : ""}`} data-hand="field" data-r="12" data-seed={floating ? 1871 : 1870} role="toolbar" aria-label="格式条">
      {!floating && (
        <BarButton label="标题" onClick={(e) => openMenu("heading", e)}>
          <Heading size={18} />
          <ChevronDown size={11} />
        </BarButton>
      )}
      {!floating && <span className="ar-sep" />}
      <BarButton label="加粗" keys="⌘B" onClick={() => apply(F.bold)}>
        <Bold size={18} />
      </BarButton>
      <BarButton label="斜体" keys="⌘I" onClick={() => apply(F.italic)}>
        <Italic size={18} />
      </BarButton>
      <BarButton label="下划线" keys="⌘U" onClick={() => apply(F.underline)}>
        <Underline size={18} />
      </BarButton>
      <BarButton label="删除线" keys="⌘⇧E" onClick={() => apply(F.strike)}>
        <Strikethrough size={18} />
      </BarButton>
      <BarButton label="高亮" keys="⌘⇧H" onClick={(e) => openMenu("highlight", e)}>
        <Highlighter size={18} />
        <span className="ar-fdot" />
      </BarButton>
      <span className="ar-sep" />
      {!floating && narrow < 2 && (
        <>
          <BarButton label="列表" onClick={() => apply(F.list)}>
            <List size={18} />
          </BarButton>
          <BarButton label="编号" onClick={() => apply(F.ordered)}>
            <ListOrdered size={18} />
          </BarButton>
          <BarButton label="待办" keys="⌘⇧L" onClick={() => apply(F.task)}>
            <ListChecks size={18} />
          </BarButton>
          <span className="ar-sep" />
        </>
      )}
      {!narrow && !floating && (
        <BarButton label="引用" onClick={() => apply(F.quote)}>
          <Quote size={18} />
        </BarButton>
      )}
      {(!narrow || floating) && (
        <BarButton label="代码" keys="⌘⇧C" onClick={() => apply(F.code)}>
          <Code size={18} />
        </BarButton>
      )}
      <BarButton label="链接" keys="⌘⇧K" onClick={() => apply(F.link)}>
        <Link2 size={18} />
      </BarButton>
      {(!narrow || floating) && (
        <BarButton label="柚子链接" keys="[[" onClick={() => apply(F.wiki)}>
          <Brackets size={18} />
        </BarButton>
      )}
      <span className="ar-sep" />
      {!floating && (
        <BarButton label="图片和附件" onClick={() => void addFiles(pickAttachments())}>
          <Paperclip size={18} />
        </BarButton>
      )}
      <BarButton label="更多格式" onClick={(e) => openMenu("more", e)}>
        <Ellipsis size={18} />
      </BarButton>
      <BarButton label={props.look.bar === "always" ? "选中文字时才出来" : "钉住格式条"} on={props.look.bar === "always"} onClick={() => props.onBar(props.look.bar === "always" ? "select" : "always")}>
        {props.look.bar === "always" ? <Pin size={18} /> : <PinOff size={18} />}
      </BarButton>
    </div>
  );

  const style = { "--ar-size": `${props.look.size}px`, "--ar-leading": props.look.leading, "--ar-para": `${props.look.para}em`, "--ar-width": props.look.width ? `${props.look.width}px` : "none" } as CSSProperties;
  // 柚子链接: asked again whenever a note or an article changes (LINKS_KEY), the article's own as a start
  const linked = useBacklinks("articles", props.article?.id ?? null).data;
  const backlinks = linked ?? props.article?.backlinks ?? [];
  return (
    <div ref={wrap} className={`ar-editor ${dropping ? "dropping" : ""}`} style={style}>
      {props.head}
      <div ref={scroller} className="ar-scroll pm-scroll" onMouseDown={onMouseDown}>
        <div className={`ar-page ${backlinks.length ? "linked" : ""}`}>
          <LiveText
            ref={live}
            boxRef={box}
            value={text}
            onChange={onChange}
            flavor={flavor}
            suggest={suggest}
            label="正文"
            placeholder="写点什么…"
            readOnly={props.readOnly}
            onKey={onKey}
            onFiles={(fs) => void addFiles(Promise.all(fs.map((f, i) => attachmentsApi.addFile(named(f, i, filesRef.current)))))}
            focusAt="end"
            className="ar-text"
          />
          <BacklinkChips items={backlinks} />
          {dropAt && <div className="ar-drop" style={{ top: dropAt.top, left: dropAt.left, width: dropAt.width }} aria-hidden />}
          {props.look.bar === "always" && !props.readOnly && <div className="h-20" aria-hidden />}
        </div>
      </div>
      {props.look.bar === "always" && !props.readOnly && <div className="ar-bar-dock">{bar(false)}</div>}
      {floatAt &&
        createPortal(
          <div className="ar-float-at" style={{ left: floatAt.left, top: floatAt.top }}>
            {bar(true)}
          </div>,
          document.body,
        )}
      {clip && <ClipBox at={clip.at} text={clip.text} onClose={() => setClip(null)} />}
      {linking && <LinkPicker at={linking.at} except={props.article ? ["articles", props.article.id] : null} onPick={(t) => linkTo(t, linking.sel)} onClose={() => setLinking(null)} />}
      {menu && <PopMenu at={menu.at} items={menuItems(menu.kind)} label={menu.kind === "heading" ? "标题" : menu.kind === "highlight" ? "高亮的颜色" : "更多格式"} onClose={() => setMenu(null)} />}
      {assist && (
        <AssistCard
          state={assist}
          at={assist.at}
          onRun={() => runAssist(assist, assist.at)}
          onClose={() => setAssist(null)}
          onUse={(out) => {
            const [s, e] = assist.from;
            if (assist.mode === "polish") edit(s, e, out, [s, s + out.length]);
            else edit(e, e, out, [e + out.length, e + out.length]);
            setAssist(null);
          }}
        />
      )}
    </div>
  );
});
