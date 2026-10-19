// 小拾's message area (behaviour from the old Picky's transcript.js):
// - Both sides render Markdown; the reply being written shows a caret; reasoning folds above a reply.
// - Actions under a message: copy (Markdown source), edit (questions) / regenerate (replies), version
//   switch "‹ 2/3 ›". The newest reply keeps its actions visible; others show them on hover. No branch
//   changes while a reply is being written. A failed / stopped / interrupted last reply offers 重试.
// - "已切换到 X" above a question answered by another model than the one before.
// - 「查了：…」 above a reply that looked things up in Pomelo; unfolds to what each lookup returned. Records
//   the lookups returned are linked where the finished reply names them; a click opens them like 全局搜索.
// - A question asked in the 小窗 shows 「带着：…」 (the page it went with), unfolding to what was sent.
// - Citations (v0.4): a finished reply's [n] become chips and the sources it cites are listed under it
//   (来源); a record opens like 全局搜索 (a past conversation opens in 小拾), a project file passage
//   shows its text with 打开文件.
// - Cards (v0.4): a reply that proposed writes shows its card under the text: one tick per item (all on);
//   an edit the words fit several records lists them to pick one; long text shows its changes marked.
//   取消 / 确认 once the reply is finished; confirming writes the ticked ones in one ⌘Z step. Afterwards
//   each written item opens its record (已撤销 when it is gone or back to how it was), unticked ones are muted.
// - Skills (v0.4 phase 6): a question that asked for some shows them (「/联网搜索」) above it; web
//   sources (联网搜索, 书单 / 影单检索) open in the browser.
// - `compact`: the 小窗's narrow column.
// - Follows new text only while the reader is at the bottom (scrolling up stops it; ↓ comes back).
// - Links open in the browser (Rust); code blocks copy; right-click a formula → 复制 LaTeX; a selection
//   spanning messages or formulas copies as plain text with formulas as written.
// - ⌘F finds in the page with the CSS Custom Highlight API (no DOM changes).
import { Fragment, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type Ref } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { ArrowDown, ChevronDown, ChevronLeft, ChevronRight, ChevronUp, Copy, FileDown, MapPin, Pencil, RotateCcw, X } from "lucide-react";
import { errorText, useToast } from "@/shared/feedback";
import { api } from "@/shared/ipc";
import { Button } from "@/shared/ui";
import { pickyApi, type Card, type CardField, type ChatMessage, type Note, type Query, type QueryRecord, type ReadingProgress, type Source } from "./api";
import { MessageFiles, openFile } from "./Files";
import { openKnowledge } from "./Knowledge";
import { EditCard, EditFileTag, WordDialog } from "./Work";
import { UsedMemories } from "./Memory";
import { DiffView } from "./Assist";
import { MarkdownView, citedIn } from "./MarkdownView";

export type TranscriptHandle = { openFind: () => void; closeFind: () => boolean; toBottom: () => void };

const RETRYABLE = new Set(["failed", "cancelled", "interrupted"]);
const COPY_SKIP = ".code-head, .caret, .katex-mathml";
const FIND_SKIP = ".code-head, .caret, .katex, button, [data-find-skip]";

function thinkingLabel(ms: number | null, live: boolean) {
  if (live) return "正在思考…";
  if (ms == null) return "思考过程";
  const s = Math.max(1, Math.round(ms / 1000));
  return s < 60 ? `已思考 ${s} 秒` : `已思考 ${Math.floor(s / 60)} 分 ${s % 60} 秒`;
}

function ActionButton(props: { label: string; onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={props.label}
      title={props.label}
      disabled={props.disabled}
      onClick={props.onClick}
      className="inline-flex size-7 items-center justify-center rounded-control text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-30"
    >
      {props.children}
    </button>
  );
}

function CopyButton(props: { text: string }) {
  const [done, setDone] = useState(false);
  const toast = useToast();
  return (
    <ActionButton
      label={done ? "已复制" : "复制"}
      onClick={() =>
        void pickyApi.copy(props.text).then(
          () => {
            setDone(true);
            setTimeout(() => setDone(false), 1200);
          },
          (e) => toast(errorText(e), "error"),
        )
      }
    >
      {done ? <span className="text-xs">✓</span> : <Copy size={14} strokeWidth={1.5} />}
    </ActionButton>
  );
}

function Versions(props: { m: ChatMessage; disabled: boolean; onSwitch: (delta: -1 | 1) => void }) {
  const { m } = props;
  if (m.versionCount <= 1) return null;
  return (
    <span className="inline-flex items-center text-xs text-muted-foreground tabular-nums">
      <ActionButton label="上一个版本" disabled={props.disabled || m.versionIndex <= 1} onClick={() => props.onSwitch(-1)}>
        <ChevronLeft size={14} strokeWidth={1.75} />
      </ActionButton>
      {m.versionIndex}/{m.versionCount}
      <ActionButton label="下一个版本" disabled={props.disabled || m.versionIndex >= m.versionCount} onClick={() => props.onSwitch(1)}>
        <ChevronRight size={14} strokeWidth={1.75} />
      </ActionButton>
    </span>
  );
}

function EditBox(props: { initial: string; onCancel: () => void; onSubmit: (text: string) => void }) {
  const [text, setText] = useState(props.initial);
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 320)}px`;
  }, [text]);
  useEffect(() => {
    const el = ref.current;
    el?.focus();
    el?.setSelectionRange(el.value.length, el.value.length);
  }, []);
  const submit = () => text.trim() && props.onSubmit(text);
  return (
    <div className="ml-auto w-full max-w-[85%] rounded-card border bg-card p-3 focus-within:ring-2 focus-within:ring-ring">
      <textarea
        ref={ref}
        value={text}
        spellCheck={false}
        aria-label="修改问题"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            props.onCancel();
          } else if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) {
            e.preventDefault();
            submit();
          }
        }}
        className="w-full resize-none bg-transparent font-reading text-[15px] leading-relaxed outline-none"
      />
      <div className="mt-2 flex justify-end gap-2">
        <Button onClick={props.onCancel}>取消</Button>
        <Button variant="primary" disabled={!text.trim()} onClick={submit}>
          发送
        </Button>
      </div>
    </div>
  );
}

function Notice(props: { m: ChatMessage; retry?: () => void }) {
  const { m } = props;
  const text =
    m.state === "failed" ? `生成失败：${m.error ?? "原因不明"}` : m.state === "cancelled" ? "已停止" : m.state === "interrupted" ? "回复没写完：上次退出时中断了" : null;
  if (!text) return null;
  return (
    <div data-find-skip className={`mt-1.5 flex items-center gap-3 text-sm ${m.state === "failed" ? "text-destructive" : "text-muted-foreground"}`}>
      <span>{text}</span>
      {props.retry && (
        <button type="button" onClick={props.retry} className="text-primary hover:underline">
          重试
        </button>
      )}
    </div>
  );
}

/** A reply reading material in segments: a progress bar and what it is reading. */
function Reading(props: { r: ReadingProgress; onStop?: () => void }) {
  const { current, total, label } = props.r;
  // 年度总结 reads its year month by month
  const monthly = /^正在读 \d+ 月$/.test(label);
  return (
    <div data-find-skip className="mb-2 max-w-md rounded-card border bg-card px-3 py-2 text-sm">
      <div className="flex items-center gap-2">
        <span className="font-bold">{monthly ? "正在读这一年" : "正在分段阅读"}</span>
        <div className="flex-1" />
        {props.onStop && (
          <button type="button" onClick={props.onStop} className="rounded-control px-1.5 text-xs text-muted-foreground hover:bg-muted hover:text-foreground">
            停止
          </button>
        )}
      </div>
      <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-muted" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={current}>
        <div className="h-full bg-primary transition-[width] duration-300" style={{ width: `${(current / Math.max(1, total)) * 100}%` }} />
      </div>
      <p className="mt-1 truncate text-xs text-muted-foreground">
        {monthly ? `第 ${current} 个月，共 ${total} 个月 · ${label}` : `第 ${current} 段，共 ${total} 段${label && ` · ${label}`}`}
      </p>
    </div>
  );
}

/** Notes from reading in segments, folded above the reply. */
function Notes(props: { notes: Note[] }) {
  return (
    <details className="picky-think group/notes mb-2" data-find-skip>
      <summary className="inline-flex cursor-default list-none items-center gap-1 text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
        <ChevronRight size={12} className="transition-transform duration-150 group-open/notes:rotate-90" />
        {props.notes.every((n) => /^\d+ 月$/.test(n.label)) ? `每月小结 · ${props.notes.length} 个月` : `分段要点 · ${props.notes.length} 段`}
      </summary>
      <div className="mt-1 space-y-2 border-l-2 pl-3">
        {props.notes.map((n, i) => (
          <section key={`${n.label}-${i}`}>
            <p className="text-xs text-muted-foreground">{n.label}</p>
            <MarkdownView className="reasoning" source={n.content} />
          </section>
        ))}
      </div>
    </details>
  );
}

/** Above a reply: what it looked up (one line), unfolding to what came back. */
function Queries(props: { queries: Query[] }) {
  return (
    <details className="picky-think group/query mb-2" data-find-skip>
      <summary className="flex max-w-full cursor-default list-none items-center gap-1 text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
        <ChevronRight size={12} className="shrink-0 transition-transform duration-150 group-open/query:rotate-90" />
        <span className="truncate">查了：{props.queries.map((q) => q.label).join("；")}</span>
      </summary>
      <div className="mt-1 space-y-2 border-l-2 pl-3">
        {props.queries.map((q, i) => (
          <section key={i}>
            <p className="text-xs text-muted-foreground">{q.label}</p>
            {q.result && <p className="selectable mt-0.5 font-reading text-[12.5px] leading-relaxed whitespace-pre-wrap text-muted-foreground">{q.result}</p>}
          </section>
        ))}
      </div>
    </details>
  );
}

/** Under a reply: the sources its text cites, by number (folded until opened). */
function SourceList(props: { sources: Source[]; onOpen: (s: Source) => void }) {
  if (!props.sources.length) return null;
  return (
    <details className="group/sources mt-2 max-w-full text-xs" data-find-skip>
      <summary className="inline-flex cursor-default list-none items-center gap-1 text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
        <ChevronRight size={12} className="transition-transform duration-150 group-open/sources:rotate-90" />
        来源 · {props.sources.length}
      </summary>
      <div className="mt-1 flex max-w-full flex-col items-start gap-0.5">
        {props.sources.map((s) => (
          <button
            key={s.n}
            type="button"
            onClick={() => props.onOpen(s)}
            className="flex max-w-full items-baseline gap-1.5 rounded-control text-left text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          >
            <span className="shrink-0 rounded-full bg-muted px-1.5 tabular-nums">{s.n}</span>
            <span className="truncate">
              {s.detail && <span>{s.detail}：</span>}
              {s.title}
            </span>
          </button>
        ))}
      </div>
    </details>
  );
}

/** One item's lines: a value, 改前 → 改后, or (long text) the new text with the changes marked. */
function CardFields(props: { fields: CardField[] }) {
  return (
    <dl className="mt-0.5 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">
      {props.fields.map((f) =>
        f.long ? (
          <Fragment key={f.label}>
            <dt className="col-span-2 text-muted-foreground">{f.label}</dt>
            <dd className="col-span-2 min-w-0 text-[13px]">{f.before != null ? <DiffView from={f.before} to={f.after} /> : <p className="max-h-60 overflow-y-auto whitespace-pre-wrap">{f.after}</p>}</dd>
          </Fragment>
        ) : (
          <Fragment key={f.label}>
            <dt className="text-muted-foreground">{f.label}</dt>
            <dd className="min-w-0 break-words">
              {f.before != null && <s className="text-muted-foreground">{f.before}</s>}
              {f.before != null && " → "}
              {f.after}
            </dd>
          </Fragment>
        ),
      )}
    </dl>
  );
}

/** Under a reply that proposed writes: one tick per item (several records fit: pick one), then 取消 / 确认. */
function ProposalCard(props: { card: Card; streaming: boolean; onOpen: (r: QueryRecord) => void }) {
  const { card } = props;
  const qc = useQueryClient();
  const toast = useToast();
  const [off, setOff] = useState<Set<number>>(() => new Set());
  const [chosen, setChosen] = useState<Map<number, number>>(() => new Map());
  const [busy, setBusy] = useState(false);
  const pending = card.state === "pending";
  const ticked = card.items.filter((i) => !off.has(i.position));
  const unpicked = ticked.some((i) => i.options && !chosen.has(i.position));
  const decide = async (confirm: boolean) => {
    setBusy(true);
    try {
      await (confirm ? pickyApi.confirmCard(card.runId, ticked.map((i) => ({ position: i.position, option: chosen.get(i.position) ?? null }))) : pickyApi.cancelCard(card.runId));
      await qc.invalidateQueries(); // the records show in their modules too
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setBusy(false);
    }
  };
  const toggle = (p: number) => setOff((s) => (s.has(p) ? new Set([...s].filter((x) => x !== p)) : new Set(s).add(p)));
  return (
    <div data-find-skip data-card={card.runId} className="mt-2 max-w-md rounded-card border bg-card text-sm">
      <ul className="divide-y">
        {card.items.map((it) => {
          const w = it.written;
          const choosing = pending && it.options;
          const pick = chosen.get(it.position);
          const fields = choosing ? (pick != null ? it.options![pick].fields : []) : it.fields;
          return (
            <li key={it.position} className={`flex items-start gap-2 px-3 py-2 ${!pending && !w ? "text-muted-foreground" : ""}`}>
              <input
                type="checkbox"
                checked={pending ? !off.has(it.position) : it.checked}
                disabled={!pending || busy}
                onChange={() => toggle(it.position)}
                aria-label={`${it.label}：${it.title}`}
                className="mt-1 size-3.5 shrink-0 accent-[var(--primary)]"
              />
              <div className="min-w-0 flex-1">
                <p className="font-bold">{it.label}</p>
                {choosing && (
                  <div role="radiogroup" aria-label={`${it.label}：选是哪一条`} className="mt-1 flex flex-col gap-1 text-xs">
                    {it.options!.map((o, k) => (
                      <label key={k} className="flex items-start gap-1.5">
                        <input
                          type="radio"
                          name={`card-${card.runId}-${it.position}`}
                          checked={pick === k}
                          disabled={busy}
                          onChange={() => setChosen((m) => new Map(m).set(it.position, k))}
                          className="mt-0.5 size-3 shrink-0 accent-[var(--primary)]"
                        />
                        <span className="min-w-0 break-words">{o.title.split("：")[0]}</span>
                      </label>
                    ))}
                  </div>
                )}
                {!pending && it.options && !w && <p className="mt-0.5 text-xs">{it.title}</p>}
                {fields.length > 0 && <CardFields fields={fields} />}
              </div>
              {w &&
                (w.gone ? (
                  <span className="shrink-0 text-xs text-muted-foreground">已撤销</span>
                ) : (
                  <button
                    type="button"
                    onClick={() => props.onOpen({ module: w.module, kind: w.kind, id: w.id, title: it.title, parent: w.parent, day: w.day })}
                    className="shrink-0 rounded-control text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    {it.fields.some((f) => f.before != null) ? "已改好" : "已记下"}
                  </button>
                ))}
            </li>
          );
        })}
      </ul>
      <div className="flex items-center justify-end gap-2 border-t px-3 py-2">
        {pending ? (
          <>
            <Button disabled={busy || props.streaming} onClick={() => void decide(false)}>
              取消
            </Button>
            <Button variant="primary" disabled={busy || props.streaming || !ticked.length || unpicked} onClick={() => void decide(true)}>
              确认
            </Button>
          </>
        ) : (
          <span className="text-xs text-muted-foreground">{card.state === "confirmed" ? "已确认" : "已取消"}</span>
        )}
      </div>
    </div>
  );
}

/** A cited project file or 知识库 passage: its text, and the whole file (a 知识库 one opens where it lives). */
function PassageDialog(props: { source: Source; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const toast = useToast();
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const s = props.source;
  return (
    <dialog
      ref={ref}
      aria-label={s.title}
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      onClick={(e) => e.target === ref.current && props.onClose()}
      className="m-auto h-fit max-h-[80vh] w-[560px] max-w-[calc(100vw-32px)] rounded-card border bg-popover p-5 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <p className="text-base font-bold">{s.title}</p>
      <p className="selectable -m-1 mt-3 max-h-[56vh] overflow-y-auto p-1 font-reading text-sm leading-relaxed whitespace-pre-wrap">{s.content}</p>
      <div className="mt-4 flex justify-end gap-2">
        {s.kbSource != null ? (
          <Button onClick={() => openKnowledge(s.kbSource!, s.kbPath ?? "", toast)}>{s.kbPath ? "打开文件" : "打开网页"}</Button>
        ) : (
          s.artifactId != null && <Button onClick={() => openFile(s.artifactId!, toast)}>打开文件</Button>
        )}
        <Button variant="primary" onClick={props.onClose}>
          关闭
        </Button>
      </div>
    </dialog>
  );
}

/** Above a question from the 小窗: the page it went with. */
function PageChip(props: { page: Query }) {
  return (
    <details className="picky-think group/page mb-1 flex max-w-[85%] flex-col items-end" data-find-skip>
      <summary className="flex max-w-full cursor-default list-none items-center gap-1 text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
        <MapPin size={12} strokeWidth={1.75} className="shrink-0" />
        <span className="truncate">带着：{props.page.label}</span>
      </summary>
      {props.page.result && <p className="selectable mt-1 border-r-2 pr-3 text-right font-reading text-[12.5px] leading-relaxed whitespace-pre-wrap text-muted-foreground">{props.page.result}</p>}
    </details>
  );
}

type ItemProps = {
  m: ChatMessage;
  divider: string | null;
  pinned: boolean;
  busy: boolean;
  last: boolean;
  editing: boolean;
  onEdit: () => void;
  onCancelEdit: () => void;
  onSubmitEdit: (text: string) => void;
  onRegenerate: () => void;
  onSwitch: (delta: -1 | 1) => void;
  onOpenFile: (id: number) => void;
  onOpenSource: (s: Source) => void;
  onOpenRecord: (r: QueryRecord) => void;
  onStop?: () => void;
};

function Item(p: ItemProps) {
  const { m } = p;
  const qc = useQueryClient();
  const [word, setWord] = useState(false);
  const links = useMemo(() => (m.queries ?? []).flatMap((q) => q.records), [m.queries]);
  const streaming = m.state === "streaming";
  const divider = p.divider && (
    <div data-find-skip className="flex items-center gap-3 text-xs text-muted-foreground">
      <span className="h-px flex-1 bg-border" />
      {p.divider}
      <span className="h-px flex-1 bg-border" />
    </div>
  );
  if (m.role === "user")
    return (
      <>
        {divider}
        <div className="group flex flex-col items-end" data-msg={m.id}>
          {m.page && <PageChip page={m.page} />}
          {(m.skills?.length ?? 0) > 0 && (
            <p data-find-skip className="mb-1 text-xs text-muted-foreground">
              {m.skills!.map((s) => `/${s}`).join(" ")}
            </p>
          )}
          {m.editFile && <EditFileTag target={m.editFile} />}
          <MessageFiles files={m.files ?? []} onOpen={p.onOpenFile} />
          {p.editing ? (
            <EditBox initial={m.content} onCancel={p.onCancelEdit} onSubmit={p.onSubmitEdit} />
          ) : (
            <div className="max-w-[85%] rounded-card bg-accent px-4 py-2.5">
              <MarkdownView body source={m.content} />
            </div>
          )}
          {!p.editing && (
            <div className="msg-actions mt-1 flex items-center opacity-0 transition-opacity duration-150 group-focus-within:opacity-100 group-hover:opacity-100">
              <Versions m={m} disabled={p.busy} onSwitch={p.onSwitch} />
              <ActionButton label="修改问题" disabled={p.busy} onClick={p.onEdit}>
                <Pencil size={14} strokeWidth={1.5} />
              </ActionButton>
              <CopyButton text={m.content} />
            </div>
          )}
        </div>
      </>
    );
  const thinkingLive = streaming && !m.content;
  return (
    <div className="group" data-msg={m.id}>
      {(m.queries?.length ?? 0) > 0 && <Queries queries={m.queries} />}
      {(m.notes?.length ?? 0) > 0 && <Notes notes={m.notes} />}
      {m.reading && <Reading r={m.reading} onStop={p.onStop} />}
      {m.reasoning && (
        <details className="picky-think group/think mb-2" data-find-skip>
          <summary className="inline-flex cursor-default list-none items-center gap-1 text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
            <ChevronRight size={12} className="transition-transform duration-150 group-open/think:rotate-90" />
            {thinkingLabel(m.reasoningMs, thinkingLive)}
          </summary>
          <MarkdownView className="reasoning mt-1 border-l-2 pl-3" source={m.reasoning} streaming={thinkingLive} />
        </details>
      )}
      {(m.content || !streaming) && <MarkdownView body source={m.content} streaming={streaming} links={streaming ? undefined : links} cites={streaming ? undefined : m.sources} />}
      {m.card && <ProposalCard card={m.card} streaming={streaming} onOpen={p.onOpenRecord} />}
      {m.edit && <EditCard edit={m.edit} streaming={streaming} onChanged={() => void qc.invalidateQueries({ queryKey: ["picky", "messages"] })} />}
      {!streaming && <SourceList sources={citedIn(m.content, m.sources ?? [])} onOpen={p.onOpenSource} />}
      {streaming && !m.content && !m.reasoning && !m.reading && <p className="animate-pulse font-reading text-sm text-muted-foreground">{m.queued ? (m.model?.startsWith("cloud:") ? "云端模型忙，等一下再试…" : "排队中（本地模型正在回答别的）") : "正在生成…"}</p>}
      {!streaming && <UsedMemories memories={m.memories ?? []} />}
      <Notice m={m} retry={p.last && !p.busy && RETRYABLE.has(m.state) ? p.onRegenerate : undefined} />
      {!streaming && (
        <div
          className={`msg-actions mt-1 -ml-1.5 flex items-center transition-opacity duration-150 ${p.pinned ? "" : "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"}`}
        >
          <CopyButton text={m.content} />
          <ActionButton label="保存为 Word" disabled={!m.content.trim() || m.state !== "complete"} onClick={() => setWord(true)}>
            <FileDown size={14} strokeWidth={1.5} />
          </ActionButton>
          <ActionButton label="重新生成" disabled={p.busy} onClick={p.onRegenerate}>
            <RotateCcw size={14} strokeWidth={1.5} />
          </ActionButton>
          <Versions m={m} disabled={p.busy} onSwitch={p.onSwitch} />
        </div>
      )}
      {word && <WordDialog messageId={m.id} onClose={() => setWord(false)} />}
    </div>
  );
}

/** "已切换到 X" goes on a question whose reply came from another model than the previous reply. */
function dividers(ms: ChatMessage[]) {
  const out = new Map<number, string>();
  let prev: string | null = null;
  ms.forEach((m, i) => {
    if (m.role !== "user") return;
    const next = ms[i + 1];
    const model = next?.role === "assistant" ? next.model : null;
    if (!model) return;
    if (prev && prev !== model) out.set(m.id, `已切换到 ${model}`);
    prev = model;
  });
  return out;
}

// ---------- selection copy ----------

/** The selected part of one message body as plain text; a partly selected formula counts whole. */
function bodyText(range: Range, body: Element, holderParent: Element) {
  const part = document.createRange();
  part.selectNodeContents(body);
  if (range.compareBoundaryPoints(Range.START_TO_START, part) > 0) part.setStart(range.startContainer, range.startOffset);
  if (range.compareBoundaryPoints(Range.END_TO_END, part) < 0) part.setEnd(range.endContainer, range.endOffset);
  if (part.collapsed) return "";
  const fragment = part.cloneContents();
  fragment.querySelectorAll("[data-source]").forEach((n) => n.replaceWith(document.createTextNode(n.getAttribute("data-source") ?? "")));
  fragment.querySelectorAll(COPY_SKIP).forEach((n) => n.remove());
  const holder = document.createElement("div");
  holder.className = "md";
  holder.style.cssText = "position:absolute;left:-99999px;top:0;width:600px";
  holder.appendChild(fragment);
  holderParent.appendChild(holder);
  const text = holder.innerText;
  holder.remove();
  return text.replace(/\n{3,}/g, "\n\n").trim();
}

// ---------- find ----------

function findRanges(root: HTMLElement, query: string) {
  const out: Range[] = [];
  if (!query) return out;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (n) => (!n.nodeValue || !n.parentElement || n.parentElement.closest(FIND_SKIP) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
  });
  const folded = query.toLowerCase();
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const value = n.nodeValue!;
    let hay = value.toLowerCase();
    let needle = folded;
    if (hay.length !== value.length || needle.length !== query.length) [hay, needle] = [value, query];
    for (let at = hay.indexOf(needle); at !== -1; at = hay.indexOf(needle, at + needle.length)) {
      const r = document.createRange();
      r.setStart(n, at);
      r.setEnd(n, at + needle.length);
      out.push(r);
    }
  }
  return out;
}

const highlights = () => ("highlights" in CSS ? CSS.highlights : null);
function paintFind(ranges: Range[], index: number) {
  const h = highlights();
  if (!h) return;
  h.set("picky-find", new Highlight(...ranges));
  if (ranges[index]) h.set("picky-find-current", new Highlight(ranges[index]));
  else h.delete("picky-find-current");
}
function clearFind() {
  highlights()?.delete("picky-find");
  highlights()?.delete("picky-find-current");
}

function FindBar(props: { query: string; count: number; index: number; onQuery: (q: string) => void; onStep: (d: 1 | -1) => void; onClose: () => void }) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  return (
    <div className="absolute top-1 right-3 z-20 flex items-center gap-1 rounded-control border bg-popover py-1 pr-1 pl-2.5 shadow-lg">
      <input
        ref={ref}
        value={props.query}
        spellCheck={false}
        aria-label="在对话里查找"
        onChange={(e) => props.onQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            props.onStep(e.shiftKey ? -1 : 1);
          } else if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            props.onClose();
          }
        }}
        className="w-44 bg-transparent text-sm outline-none"
      />
      <span className="min-w-10 text-right text-xs text-muted-foreground tabular-nums">{props.query ? (props.count ? `${props.index + 1}/${props.count}` : "0/0") : ""}</span>
      <ActionButton label="上一个" disabled={!props.count} onClick={() => props.onStep(-1)}>
        <ChevronUp size={14} strokeWidth={1.75} />
      </ActionButton>
      <ActionButton label="下一个" disabled={!props.count} onClick={() => props.onStep(1)}>
        <ChevronDown size={14} strokeWidth={1.75} />
      </ActionButton>
      <ActionButton label="关闭查找" onClick={props.onClose}>
        <X size={14} strokeWidth={1.75} />
      </ActionButton>
    </div>
  );
}

// ---------- the transcript ----------

export function Transcript(props: {
  ref?: Ref<TranscriptHandle>;
  /** Changes when another conversation opens (scrolls to the bottom, closes editors). */
  sessionKey: string;
  messages: ChatMessage[];
  busy: boolean;
  empty: ReactNode;
  onEdit: (m: ChatMessage, text: string) => void;
  onRegenerate: (m: ChatMessage) => void;
  onSwitch: (m: ChatMessage, delta: -1 | 1) => void;
  onOpenFile: (id: number) => void;
  /** Stops the reply being written (from its reading progress). */
  onStop?: () => void;
  /** Jump to a message (a search hit): scroll there, flash it, mark the words for a few seconds. */
  reveal?: { id: number; query: string; key: number } | null;
  /** The 小窗's narrow column. */
  compact?: boolean;
}) {
  const { messages, busy } = props;
  const toast = useToast();
  const scroller = useRef<HTMLDivElement>(null);
  const column = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const [awayFromBottom, setAway] = useState(false);
  const [editing, setEditing] = useState<number | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; tex: string } | null>(null);
  const [find, setFind] = useState<{ query: string; ranges: Range[]; index: number } | null>(null);
  const [passage, setPassage] = useState<Source | null>(null);

  const distance = () => {
    const el = scroller.current;
    return el ? el.scrollHeight - el.scrollTop - el.clientHeight : 0;
  };
  const toBottom = useCallback(() => {
    following.current = true;
    const el = scroller.current;
    if (el) el.scrollTop = el.scrollHeight;
    setAway(false);
  }, []);

  // another conversation: start at the bottom, nothing being edited
  useLayoutEffect(() => {
    setEditing(null);
    toBottom();
  }, [props.sessionKey, toBottom]);
  // new text: follow only while at the bottom
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && following.current) el.scrollTop = el.scrollHeight;
  });

  // ----- find -----
  const runFind = useCallback((query: string, keep?: number) => {
    const root = column.current;
    if (!root) return;
    const ranges = findRanges(root, query);
    let index = 0;
    if (keep !== undefined) index = Math.min(keep, Math.max(0, ranges.length - 1));
    else {
      const top = scroller.current?.getBoundingClientRect().top ?? 0;
      const firstInView = ranges.findIndex((r) => r.getBoundingClientRect().top >= top);
      index = firstInView < 0 ? 0 : firstInView;
    }
    paintFind(ranges, index);
    setFind({ query, ranges, index });
    return { ranges, index };
  }, []);
  const reveal = (r: Range | undefined) => {
    const el = scroller.current;
    if (!r || !el) return;
    const rect = r.getBoundingClientRect();
    const box = el.getBoundingClientRect();
    following.current = false;
    el.scrollTop += rect.top - box.top - box.height / 2;
    setAway(distance() > 80);
  };
  const onQuery = (query: string) => {
    const res = runFind(query);
    reveal(res?.ranges[res.index]);
  };
  const step = (d: 1 | -1) => {
    if (!find || !find.ranges.length) return;
    const index = (find.index + d + find.ranges.length) % find.ranges.length;
    paintFind(find.ranges, index);
    setFind({ ...find, index });
    reveal(find.ranges[index]);
  };
  const closeFind = () => {
    clearFind();
    setFind(null);
  };
  // content changed under an open find: re-mark without moving the reader
  const findQuery = find?.query;
  const findIndex = find?.index;
  const contentKey = messages.map((m) => `${m.id}:${m.state}:${m.content.length}:${m.reasoning.length}`).join("|");
  useEffect(() => {
    if (!findQuery) return;
    const t = setTimeout(() => runFind(findQuery, findIndex), 400);
    return () => clearTimeout(t);
  }, [contentKey, findQuery]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => clearFind, []);

  // ----- reveal (search hit): waits until the message is on screen -----
  const revealed = useRef<number | null>(null);
  useEffect(() => {
    const r = props.reveal;
    if (!r || revealed.current === r.key) return;
    const el = column.current?.querySelector<HTMLElement>(`[data-msg="${r.id}"]`);
    if (!el) return;
    revealed.current = r.key;
    following.current = false;
    el.scrollIntoView({ block: "center" });
    setAway(distance() > 80);
    el.classList.remove("picky-flash");
    void el.offsetWidth;
    el.classList.add("picky-flash");
    const ranges = r.query ? r.query.split(/\s+/).filter(Boolean).flatMap((w) => findRanges(el, w)) : [];
    if (ranges.length) highlights()?.set("picky-reveal", new Highlight(...ranges));
    setTimeout(() => {
      highlights()?.delete("picky-reveal");
      el.classList.remove("picky-flash");
    }, 6000);
  });

  useImperativeHandle(props.ref, () => ({
    openFind: () => setFind((f) => f ?? { query: "", ranges: [], index: 0 }),
    closeFind: () => {
      if (!find) return false;
      closeFind();
      return true;
    },
    toBottom,
  }));

  // ----- copy: a selection across messages or formulas → plain text, formulas as written -----
  useEffect(() => {
    const onCopy = (e: ClipboardEvent) => {
      const root = column.current;
      const sel = window.getSelection();
      if (!root || !sel || sel.isCollapsed || !sel.rangeCount) return;
      const range = sel.getRangeAt(0);
      if (!root.contains(range.commonAncestorContainer)) return;
      const bodies = [...root.querySelectorAll("[data-body]")].filter((b) => range.intersectsNode(b));
      const math = bodies.some((b) => [...b.querySelectorAll("[data-source]")].some((n) => range.intersectsNode(n)));
      if (bodies.length < 2 && !math) return;
      const text = bodies
        .map((b) => bodyText(range, b, root))
        .filter(Boolean)
        .join("\n\n");
      if (!text) return;
      e.clipboardData?.setData("text/plain", text);
      e.preventDefault();
    };
    document.addEventListener("copy", onCopy);
    return () => document.removeEventListener("copy", onCopy);
  }, []);

  // ----- clicks inside messages: records, links, code copy -----
  const navigate = useNavigate();
  const openHit = (r: QueryRecord) => void navigate(`/${r.module}`, { state: { open: { ...r, detail: null, parent: r.parent ?? null, day: r.day ?? null, amount: null, currency: null, flow: null } } });
  const openRecord = (el: HTMLElement) => {
    const r = JSON.parse(el.dataset.record ?? "null") as QueryRecord | null;
    if (r) openHit(r);
  };
  const openSource = (s: Source) => {
    if (s.kind === "passage") return setPassage(s);
    if (s.kind === "web") return void (s.url && api.openLink(s.url).catch((e) => toast(errorText(e), "error")));
    if (!s.module || !s.recordKind || s.id == null) return;
    // a past conversation opens in 小拾 (on the message when it is one)
    if (s.module === "picky") return openHit({ module: "picky", kind: "chat", id: s.recordKind === "message" ? (s.parent ?? 0) : s.id, title: s.title, parent: s.recordKind === "message" ? s.id : null });
    openHit({ module: s.module, kind: s.recordKind, id: s.id, title: s.title, parent: s.parent, day: s.day });
  };
  const openCite = (el: HTMLElement) => {
    const s = JSON.parse(el.dataset.cite ?? "null") as Source | null;
    if (s) openSource(s);
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    const t = e.target as HTMLElement;
    const rec = t.closest?.(".record-link") as HTMLElement | null;
    const cite = t.closest?.(".cite-link") as HTMLElement | null;
    if ((rec || cite) && e.key === "Enter") {
      e.preventDefault();
      if (rec) openRecord(rec);
      else if (cite) openCite(cite);
    }
  };
  const onClick = (e: React.MouseEvent) => {
    const t = e.target as HTMLElement;
    const rec = t.closest(".record-link") as HTMLElement | null;
    if (rec) {
      openRecord(rec);
      return;
    }
    const cite = t.closest(".cite-link") as HTMLElement | null;
    if (cite) {
      openCite(cite);
      return;
    }
    const link = t.closest("a[href]") as HTMLAnchorElement | null;
    if (link && link.closest(".md")) {
      e.preventDefault();
      void api.openLink(link.getAttribute("href")!).catch((err) => toast(errorText(err), "error"));
      return;
    }
    const copy = t.closest('button[data-action="copy-code"]') as HTMLButtonElement | null;
    if (copy) {
      const code = copy.closest(".code-block")?.querySelector("code")?.textContent ?? "";
      void pickyApi.copy(code.replace(/\n$/, "")).then(() => {
        const label = copy.querySelector("span");
        if (!label || copy.dataset.done) return;
        copy.dataset.done = "1";
        label.textContent = "已复制";
        setTimeout(() => {
          label.textContent = "复制";
          delete copy.dataset.done;
        }, 1200);
      });
    }
  };
  const onContextMenu = (e: React.MouseEvent) => {
    const formula = (e.target as HTMLElement).closest?.(".math, .math-display");
    if (!formula) return;
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, tex: formula.getAttribute("data-tex") ?? "" });
  };
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", onKey);
    scroller.current?.addEventListener("scroll", close, { once: true });
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu]);

  const divs = dividers(messages);
  const lastReply = [...messages].reverse().find((m) => m.role === "assistant")?.id;
  const lastId = messages[messages.length - 1]?.id;

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      {find && <FindBar query={find.query} count={find.ranges.length} index={find.index} onQuery={onQuery} onStep={step} onClose={closeFind} />}
      <div
        ref={scroller}
        onScroll={() => {
          const d = distance();
          if (d <= 4) following.current = true;
          else if (d > 60) following.current = false;
          setAway(!following.current && d > 80);
        }}
        onWheel={(e) => {
          if (e.deltaY < 0 && distance() > 0) following.current = false;
        }}
        onClick={onClick}
        onKeyDown={onKeyDown}
        onContextMenu={onContextMenu}
        className="-m-1 min-h-0 flex-1 overflow-y-auto p-1"
      >
        {messages.length === 0 ? (
          <div className="flex h-full items-center justify-center pb-16 text-lg">{props.empty}</div>
        ) : (
          <div ref={column} className={`relative mx-auto max-w-3xl space-y-6 pt-2 pb-6 ${props.compact ? "px-4" : "px-8"}`}>
            {messages.map((m) => (
              <Item
                key={m.id}
                m={m}
                divider={divs.get(m.id) ?? null}
                pinned={m.id === lastReply}
                busy={busy}
                last={m.id === lastId}
                editing={editing === m.id}
                onEdit={() => setEditing(m.id)}
                onCancelEdit={() => setEditing(null)}
                onSubmitEdit={(text) => {
                  setEditing(null);
                  props.onEdit(m, text);
                }}
                onRegenerate={() => props.onRegenerate(m)}
                onSwitch={(d) => props.onSwitch(m, d)}
                onOpenFile={props.onOpenFile}
                onOpenSource={openSource}
                onOpenRecord={openHit}
                onStop={props.onStop}
              />
            ))}
          </div>
        )}
      </div>
      {awayFromBottom && (
        <button
          type="button"
          aria-label="回到底部"
          title="回到底部"
          onClick={toBottom}
          className="absolute bottom-3 left-1/2 z-10 flex size-8 -translate-x-1/2 items-center justify-center rounded-full border bg-popover text-muted-foreground shadow-md hover:text-foreground"
        >
          <ArrowDown size={15} strokeWidth={1.75} />
        </button>
      )}
      {menu && (
        <div
          role="menu"
          style={{ position: "fixed", left: menu.x, top: menu.y }}
          onMouseDown={(e) => e.stopPropagation()}
          className="z-50 animate-pop-in rounded-card border bg-popover p-1 text-sm shadow-lg"
        >
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              void pickyApi.copy(menu.tex);
              setMenu(null);
            }}
            className="rounded-md px-3 py-1.5 hover:bg-muted"
          >
            复制 LaTeX
          </button>
        </div>
      )}
      {passage && <PassageDialog source={passage} onClose={() => setPassage(null)} />}
    </div>
  );
}
