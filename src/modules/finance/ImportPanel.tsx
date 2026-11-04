// 导入账单 (Importer: Rust picky/assistants/importer.rs; the rows finance/import.rs; requirements「记账 · 导入账单」):
// - 导入: the files go to the chosen model, which reads every transaction out (Excel, PDF, screenshots) and checks
//   them against 收支 and earlier statements. It runs on while this page is closed (importJob.ts). 提示词清单: my
//   own points, those ticked go with every request. The model, its 思考 and the 清单 are Importer's settings, kept by Rust; no
//   cloud model, or 收支 not for the cloud, and there is nothing to read with (what's here stays usable).
// - Each file is a batch of rows 待核对, listed on the left (kept after 入账; drag to order, ⋯ 改名 / 删除:
//   its entries stay). 再整理: the rows I haven't saved (the ticked ones, if any) again under the 清单.
// - Ticking: the box, the header box (all shown), shift for a range, ⌘ on a row, or drag down the rows. Ticked rows
//   change 分类 together, and go in with 入账, one ⌘Z step each.
// - A row opens on the right (× closes): its fields, the screenshot (held only while the app is open), possible
//   duplicates, 忽略.
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { Bird, ChevronDown, ChevronRight, Ellipsis, FileDown, Maximize2, Plus, Sparkles, X } from "lucide-react";
import { useSortable } from "@/shared/sortable";
import { DateInput } from "@/shared/DateInput";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { KINDS_KEY, UNCATEGORIZED, useKinds } from "@/shared/kinds/api";
import { Button, Field, IconButton, TextInput } from "@/shared/ui";
import { pickyApi, usePickyStatus } from "@/modules/picky/api";
import { LevelPicker } from "@/modules/picky/Thinking";
import { FIN_KEY, IN_KINDS, OUT_KINDS } from "./api";
import { AmountInput, CurrencyPicker } from "./fields";
import { importApi, importerApi, TREATMENTS, useRefundCandidates, type BatchSummary, type ImportDraft, type ImporterPoint, type ImporterSettings, type ImportRow } from "./importApi";
import { importJobPreview, importJobTake, runImport, runTidy, useImportJob } from "./importJob";
import { CURRENCIES, CURRENCY_NAME, money, moneyInput, parseMoney } from "./money";

const SETTINGS = ["importer"];
const NO_FILTERS = { from: "", to: "", currency: "", treatment: "", category: "", text: "" };
const open_ = (s: string) => s === "pending" || s === "ready";
const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
/** Duplicates still to decide. */
const undecided = (r: ImportRow) => r.resolution !== "new" && ((r.pendingCandidates?.length ?? 0) > 0 || r.candidates.length > 0 || ["review", "error"].includes(r.ai?.duplicate ?? ""));
/** 疑点 the model hasn't settled (its own question counts too). */
const doubtful = (r: ImportRow) => !r.ai?.settled && (r.source.issues.length > 0 || !!r.ai?.why);
const tickable = (r: ImportRow) => open_(r.state);
const menuTrigger =
  "rounded-control p-0.5 text-muted-foreground opacity-0 outline-none group-hover:opacity-100 hover:bg-muted hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring aria-expanded:opacity-100";

function rowState(r: ImportRow) {
  if (r.state === "duplicate") return "重复";
  if (r.state === "posted") return r.entryId ? "已入账" : "记录已删";
  if (r.state === "ignored") return "已忽略";
  if (r.state === "neutral") return "不计收支";
  if (undecided(r)) return "可能重复";
  if (r.state === "ready") return "已核对";
  return doubtful(r) ? "有疑点" : "待确认";
}

/** Ready to go in without a look: nothing odd, no duplicate to decide. */
const clean = (r: ImportRow) => tickable(r) && !undecided(r) && (r.state === "ready" || !doubtful(r));

function NameInput(props: { initial: string; label: string; max: number; onDone: (name: string | null) => void }) {
  const [v, setV] = useState(props.initial);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => ref.current?.select(), []);
  return (
    <TextInput
      ref={ref}
      aria-label={props.label}
      value={v}
      maxLength={props.max}
      onChange={(e) => setV(e.target.value)}
      onKeyDown={(e) => {
        if (e.nativeEvent.isComposing) return;
        if (e.key === "Enter") props.onDone(v);
        if (e.key === "Escape") props.onDone(null);
      }}
      onBlur={() => props.onDone(v)}
    />
  );
}

/** 提示词清单: a line each; the ticked ones go to the model. Folded, the line says which are in force. */
function Checklist(props: { points: ImporterPoint[]; disabled: boolean; onChange: (points: ImporterPoint[]) => void }) {
  const { points } = props;
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState<number | "new" | null>(null);
  const on = points.filter((p) => p.on);
  const Arrow = open ? ChevronDown : ChevronRight;
  const edit = (at: number | "new", text: string | null) => {
    setEditing(null);
    if (text === null) return;
    const t = text.trim();
    if (at === "new") {
      if (t) props.onChange([...points, { text: t, on: true }]);
    } else if (!t) props.onChange(points.filter((_, i) => i !== at));
    else if (t !== points[at].text) props.onChange(points.map((p, i) => (i === at ? { ...p, text: t } : p)));
  };
  return (
    <div className="shrink-0 text-sm">
      <button type="button" aria-expanded={open} onClick={() => setOpen(!open)} className="flex w-full items-center gap-1.5 rounded-control py-1 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <Arrow size={14} className="shrink-0 text-muted-foreground" />
        <span className="shrink-0">提示词清单</span>
        <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
          {on.length} / {points.length}
        </span>
        {!open && <span className="min-w-0 truncate text-xs text-muted-foreground">{on.map((p) => p.text).join("；")}</span>}
      </button>
      {open && (
        <ul className="mt-1 max-h-52 overflow-y-auto rounded-card border bg-card p-1.5">
          {points.map((p, i) => (
            <li key={i} className="group flex items-center gap-2 rounded-control px-1.5 py-0.5 hover:bg-muted/60">
              <input aria-label="这次生效" type="checkbox" disabled={props.disabled} checked={p.on} onChange={() => props.onChange(points.map((x, k) => (k === i ? { ...x, on: !x.on } : x)))} className="size-4 shrink-0 accent-[var(--primary)]" />
              {editing === i ? (
                <div className="min-w-0 flex-1">
                  <NameInput initial={p.text} label="要点" max={500} onDone={(t) => edit(i, t)} />
                </div>
              ) : (
                <button type="button" disabled={props.disabled} onClick={() => setEditing(i)} className={`min-w-0 flex-1 rounded-control py-0.5 text-left break-words outline-none focus-visible:ring-2 focus-visible:ring-ring ${p.on ? "" : "text-muted-foreground"}`}>
                  {p.text}
                </button>
              )}
              <span className="opacity-0 group-hover:opacity-100 focus-within:opacity-100">
                <IconButton label="删除" disabled={props.disabled} onClick={() => props.onChange(points.filter((_, k) => k !== i))}>
                  <X size={14} />
                </IconButton>
              </span>
            </li>
          ))}
          <li className="px-1.5 py-0.5">
            {editing === "new" ? (
              <NameInput initial="" label="新的要点" max={500} onDone={(t) => edit("new", t)} />
            ) : (
              <Button variant="ghost" className="px-1.5 py-0.5 text-xs" disabled={props.disabled} onClick={() => setEditing("new")}>
                <Plus size={12} /> 加一条
              </Button>
            )}
          </li>
        </ul>
      )}
    </div>
  );
}

function ReviewRow(props: { row: ImportRow; preview?: string; dirty: boolean; onSaved: () => void; onDirty: (dirty: boolean) => void; onClose: () => void }) {
  const row = props.row;
  const [big, setBig] = useState(false);
  const mark = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLDivElement>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    if (big) dialog.current?.showModal();
  }, [big]);
  const [draft, setDraft] = useState(row.draft);
  const [amount, setAmount] = useState(moneyInput(row.draft.amount));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { data: kinds = [] } = useKinds(draft.treatment === "in" ? IN_KINDS : OUT_KINDS);
  const refunds = useRefundCandidates(draft.currency, draft.day, draft.treatment === "refund");
  const editable = open_(row.state);
  const update = (change: Partial<ImportDraft>) => {
    setDraft({ ...draft, ...change });
    props.onDirty(true);
  };
  const act = async (f: () => Promise<unknown>, after = props.onSaved) => {
    setBusy(true);
    setError(null);
    try {
      await f();
      after();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const save = () => act(() => importApi.save(row.id, { ...draft, amount: parseMoney(amount) }), () => (props.onDirty(false), props.onSaved()));
  const held = busy || props.dirty;
  const region = row.source.region;
  const picture = props.preview && (
    <div className="relative">
      <img className="block w-full" src={props.preview} alt="" onLoad={() => frame.current?.scrollTo({ top: Math.max(0, (mark.current?.offsetTop ?? 0) - 24) })} />
      {region && (
        <div
          ref={mark}
          className="pointer-events-none absolute border-2 border-primary bg-primary/10"
          style={{ left: `${region[0] * 100}%`, top: `${region[1] * 100}%`, width: `${region[2] * 100}%`, height: `${region[3] * 100}%` }}
        />
      )}
    </div>
  );
  const ai = row.ai;
  return (
    <aside aria-label="核对" className="-m-1 w-80 shrink-0 space-y-4 overflow-y-auto border-l p-1 pl-4">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <p className="font-bold">第 {row.source.rowNumber} 行</p>
          <p className="text-xs break-all text-muted-foreground">
            {[row.source.sheet, row.source.status].filter(Boolean).join(" · ")}
          </p>
        </div>
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      {picture && (
        <div className="relative">
          <div ref={!big ? frame : undefined} className="max-h-60 overflow-auto rounded-control border">
            {!big && picture}
          </div>
          <div className="absolute top-1 right-1 rounded-control bg-card/90">
            <IconButton label="放大" onClick={() => setBig(true)}>
              <Maximize2 size={14} />
            </IconButton>
          </div>
        </div>
      )}
      {big && (
        <dialog ref={dialog} aria-label="原图" onCancel={() => setBig(false)} className="fixed inset-0 m-auto flex h-[90vh] w-[90vw] flex-col rounded-card border bg-background p-4 text-foreground shadow-2xl backdrop:bg-black/40">
          <div className="flex shrink-0 justify-end pb-2">
            <IconButton label="关闭" onClick={() => setBig(false)}>
              <X size={16} />
            </IconButton>
          </div>
          <div ref={frame} className="min-h-0 flex-1 overflow-auto">
            <div className="mx-auto max-w-3xl">{picture}</div>
          </div>
        </dialog>
      )}
      {row.source.issues.length > 0 && (
        <ul className="space-y-1 text-sm">
          {row.source.issues.map((s) => (
            <li key={s} className={`text-muted-foreground ${row.ai?.settled ? "line-through decoration-muted-foreground/60" : ""}`}>
              {s}
            </li>
          ))}
        </ul>
      )}
      {ai && (
        <div className="space-y-0.5 text-xs text-muted-foreground">
          <p className="flex items-center gap-1">
            <Sparkles size={12} /> {ai.model}
          </p>
          {ai.why && <p className={ai.settled ? "" : "text-foreground"}>{ai.why}</p>}
          {ai.noteError && <p className="text-destructive">{ai.noteError}</p>}
          {ai.reason && <p>{ai.reason}</p>}
        </div>
      )}
      {editable ? (
        <>
          <Field label="算作">
            <Dropdown ariaLabel="算作" value={draft.treatment} options={[...TREATMENTS]} onChange={(treatment) => update({ treatment, category: null, refundOf: null })} block />
          </Field>
          <Field label={draft.treatment === "refund" ? "到账日期" : "日期"}>
            <DateInput ariaLabel="日期" value={draft.day} onChange={(day) => update({ day })} />
          </Field>
          <Field label="金额">
            <div className="flex flex-wrap gap-2">
              <AmountInput
                value={amount}
                onChange={(v) => {
                  setAmount(v);
                  props.onDirty(true);
                }}
                className="w-28"
              />
              <CurrencyPicker value={draft.currency} onChange={(currency) => update({ currency, refundOf: null })} />
            </div>
          </Field>
          {draft.treatment === "refund" && (
            <Field label="原支出">
              <Dropdown
                ariaLabel="原支出"
                value={draft.refundOf}
                options={[{ value: null as number | null, label: "不关联" }, ...(refunds.data ?? []).map((e) => ({ value: e.id as number | null, label: `${e.day} ${money(e.amount, e.currency)} ${e.note ?? ""}` }))]}
                onChange={(refundOf) => update({ refundOf, category: refundOf ? (kinds.find((k) => k.id === refunds.data?.find((e) => e.id === refundOf)?.kindId)?.name ?? null) : draft.category })}
                block
              />
              {refunds.error && <p className="text-sm text-destructive">{errorText(refunds.error)}</p>}
            </Field>
          )}
          {draft.treatment !== "neutral" && (
            <Field label="分类">
              <Dropdown ariaLabel="分类" value={draft.category} options={[{ value: null as string | null, label: UNCATEGORIZED }, ...kinds.map((k) => ({ value: k.name as string | null, label: k.name }))]} onChange={(category) => update({ category })} block />
            </Field>
          )}
          <Field label="备注">
            <TextInput value={draft.note} maxLength={1000} onChange={(e) => update({ note: e.target.value })} />
          </Field>
          <div className="flex gap-2">
            <Button variant="primary" disabled={busy || !props.dirty} onClick={() => void save()}>
              保存
            </Button>
            <Button disabled={held} onClick={() => void act(() => importApi.ignore(row.id, true))}>
              忽略
            </Button>
          </div>
        </>
      ) : (
        <div className="flex items-center gap-2 text-sm">
          <span className="text-muted-foreground">{rowState(row)}</span>
          {row.state === "ignored" && (
            <Button disabled={busy} onClick={() => void act(() => importApi.ignore(row.id, false))}>
              恢复
            </Button>
          )}
        </div>
      )}
      {editable && row.candidates.length > 0 && (
        <section className="space-y-2">
          <p className="text-sm font-bold">可能记过</p>
          {row.candidates.map(({ entry, sources }) => (
            <div key={entry.id} className="flex items-start gap-2 rounded-control border p-2 text-xs">
              <div className="min-w-0 flex-1">
                <p className="tabular-nums">
                  {entry.day} · {money(entry.amount, entry.currency)}
                </p>
                <p className="truncate text-muted-foreground">{[entry.note, sources.join("、") || "手记的"].filter(Boolean).join(" · ")}</p>
              </div>
              <Button className="py-1" disabled={held} onClick={() => void act(() => importApi.resolve(row.id, entry.id))}>
                是这笔
              </Button>
            </div>
          ))}
        </section>
      )}
      {editable && (row.pendingCandidates?.length ?? 0) > 0 && (
        <section className="space-y-2">
          <p className="text-sm font-bold">别的账单里</p>
          {row.pendingCandidates!.map((c) => (
            <div key={c.id} className="flex items-start gap-2 rounded-control border p-2 text-xs">
              <div className="min-w-0 flex-1">
                <p className="tabular-nums">
                  {c.draft.day} · {money(c.draft.amount ?? 0, c.draft.currency)}
                </p>
                <p className="truncate text-muted-foreground">{[c.draft.note, c.name].filter(Boolean).join(" · ")}</p>
              </div>
              <Button className="py-1" disabled={held} onClick={() => void act(() => importApi.linkPending(row.id, c.id))}>
                是这笔
              </Button>
            </div>
          ))}
        </section>
      )}
      {editable && undecided(row) && (
        <Button disabled={held} onClick={() => void act(() => importApi.resolve(row.id, null))}>
          不是重复
        </Button>
      )}
      {row.resolution === "linked" && (
        <div className="flex items-center gap-2 text-sm">
          <span className="text-muted-foreground">已关联，只算一笔</span>
          <Button disabled={busy} onClick={() => void act(() => importApi.unlink(row.id))}>
            解除
          </Button>
        </div>
      )}
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <details>
        <summary className="cursor-pointer text-sm text-muted-foreground">原文</summary>
        <dl className="mt-2 space-y-2 text-xs">
          {row.source.raw.map(([name, value], i) => (
            <div key={i}>
              <dt className="text-muted-foreground">{name || `第 ${i + 1} 列`}</dt>
              <dd className="break-all whitespace-pre-wrap">{value || "—"}</dd>
            </div>
          ))}
        </dl>
      </details>
    </aside>
  );
}

export function ImportPanel(props: { onClose: () => void; onPosted: (day: string) => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const cloud = usePickyStatus().data?.cloud ?? [];
  const settings = useQuery({ queryKey: SETTINGS, queryFn: importerApi.settings });
  const points = settings.data?.points ?? [];
  const models = cloud.map((m) => ({ value: `cloud:${m.id}`, label: m.name }));
  const model = [settings.data?.model].find((m) => m && models.some((x) => x.value === m)) ?? models[0]?.value ?? null;
  // 收支 local only (设置 → 柚子): nothing to read with
  const flowOut = useQuery({ queryKey: ["picky", "data-cloud"], queryFn: pickyApi.dataCloud }).data?.find((m) => m.module === "finance/flow")?.allowed !== false;
  const aiOk = !!model && flowOut;
  const noAi = !flowOut ? "钱来钱往 · 收支不发给云端" : !model ? "没有云端模型" : null;
  const job = useImportJob();
  const previews = job.previews;
  const busy = job.doing;
  const saveSettings = (change: Partial<ImporterSettings>) => {
    const next = { model: settings.data?.model ?? null, level: settings.data?.level ?? "off", points, ...change };
    qc.setQueryData(SETTINGS, next);
    importerApi.save(next).then((saved) => qc.setQueryData(SETTINGS, saved), (e) => setError(errorText(e)));
  };
  const [id, setId] = useState<number | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const [dirty, setDirty] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [status, setStatus] = useState("pending");
  const [filters, setFilters] = useState(NO_FILTERS);
  useEffect(() => setSelected(new Set()), [status, filters]);
  const [error, setError] = useState<string | null>(null);
  const summaries = useQuery({ queryKey: [...FIN_KEY, "imports"], queryFn: importApi.batches });
  const activeId = id ?? summaries.data?.[0]?.id;
  const query = useQuery({ queryKey: [...FIN_KEY, "import", activeId], queryFn: () => importApi.batch(activeId!), enabled: activeId != null });
  const batch = query.data;
  const rows = batch?.rows ?? [];
  const text = filters.text.trim().toLocaleLowerCase();
  const shown = rows.filter(
    (r) =>
      (status === "all" || (status === "pending" ? open_(r.state) : status === "duplicates" ? open_(r.state) && undecided(r) : r.state === status)) &&
      (!filters.from || r.draft.day >= filters.from) &&
      (!filters.to || (!!r.draft.day && r.draft.day <= filters.to)) &&
      (!filters.currency || r.draft.currency === filters.currency) &&
      (!filters.treatment || r.draft.treatment === filters.treatment) &&
      (!filters.category || (filters.category === "none" ? !r.draft.category : r.draft.category === filters.category)) &&
      (!text || `${r.draft.note} ${JSON.stringify(r.source.raw)}`.toLocaleLowerCase().includes(text)),
  );
  const row = rows.find((r) => r.id === editing);
  const filtered = status !== "pending" || Object.values(filters).some(Boolean);
  const refresh = async () => {
    await qc.invalidateQueries({ queryKey: FIN_KEY });
    await qc.invalidateQueries({ queryKey: KINDS_KEY });
  };
  useEffect(() => {
    const taken = importJobTake();
    if (taken.notice) toast(taken.notice);
    if (taken.show != null) show(taken.show);
  }, [job.notice, job.show]); // eslint-disable-line react-hooks/exhaustive-deps
  const leave = async () => !dirty || (await confirm({ title: "不保存这一行的修改？", confirmLabel: "不保存", danger: true }));
  const show = (batchId: number, first: number | null = null) => {
    setId(batchId);
    setEditing(first);
    setDirty(false);
    setSelected(new Set());
    setStatus("pending");
    setFilters(NO_FILTERS);
  };
  const run = async (f: () => Promise<void>) => {
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const pick = async () => {
    if (!model || !(await leave())) return;
    const chosen = await open({ title: "导入账单", multiple: true, filters: [{ name: "账单", extensions: ["xlsx", "xls", "xlsm", "pdf", "png", "jpg", "jpeg"] }] });
    const picked = chosen === null ? [] : Array.isArray(chosen) ? chosen : [chosen];
    const paths: string[] = [];
    for (const path of picked) {
      const name = fileName(path);
      const old = summaries.data?.find((b) => b.name === name);
      if (old && !(await confirm({ title: `「${old.title ?? name}」导入过了，再导入一次？`, message: "再读一遍会多一份账单，重复的会列出来。", confirmLabel: "再导入" }))) continue;
      paths.push(path);
    }
    if (paths.length) await runImport(qc, paths, model);
  };
  const reTidy = async () => {
    if (!activeId || !model || !(await leave())) return;
    setEditing(null);
    setDirty(false);
    const only = rows.filter((r) => selected.has(r.id) && open_(r.state)).map((r) => r.id);
    await runTidy(qc, activeId, only.length ? only : null, model);
  };
  const add = async () => {
    if (!activeId || !(await leave())) return;
    await run(async () => {
      const added = await importApi.add(activeId);
      show(activeId, added);
      await refresh();
    });
  };
  const choosePicture = () =>
    run(async () => {
      if (!activeId) return;
      const path = await open({ title: "选原图", multiple: false, filters: [{ name: "截图", extensions: ["png", "jpg", "jpeg"] }] });
      if (typeof path !== "string") return;
      const preview = await importApi.preview(activeId, path);
      importJobPreview(activeId, preview);
    });
  const post = () =>
    run(async () => {
      if (!batch || dirty) return;
      const ids = rows.filter((r) => selected.has(r.id) && open_(r.state)).map((r) => r.id);
      const r = await importApi.commit(batch.summary.id, ids);
      const day = rows.find((x) => ids.includes(x.id))?.draft.day;
      setSelected(new Set());
      setEditing(null);
      await refresh();
      toast([`入账 ${r.posted} 笔`, r.neutral && `不计收支 ${r.neutral} 笔`].filter(Boolean).join(" · "));
      if (day) props.onPosted(day);
    });

  // the list on the left
  const [renaming, setRenaming] = useState<number | null>(null);
  const sort = useSortable<HTMLElement>((ids) => void run(async () => (await importApi.reorder(ids), await refresh())));
  const rename = (id: number, title: string | null) => {
    setRenaming(null);
    if (title !== null) void run(async () => (await importApi.rename(id, title), await refresh()));
  };
  const removeBatch = async (b: BatchSummary) => {
    if (!(await confirm({ title: `删除「${b.title ?? b.name}」？`, message: "已入账的收支留着。", confirmLabel: "删除", danger: true }))) return;
    await run(async () => {
      await importApi.remove(b.id);
      const left = (summaries.data ?? []).filter((x) => x.id !== b.id);
      qc.setQueryData([...FIN_KEY, "imports"], left);
      if (b.id === activeId) {
        if (left.length) show(left[0].id);
        else setId(null);
      }
      await refresh();
    });
  };

  // ticking: `anchor` for shift ranges; `sweep` while dragging down the rows (from its row, to tick or untick,
  // over what was ticked before it began)
  const anchor = useRef<number | null>(null);
  const sweep = useRef<{ from: number; on: boolean; base: Set<number>; moved: boolean } | null>(null);
  const tickRange = (from: number, to: number, on: boolean, base: Set<number>) => {
    const ids = shown.map((r) => r.id);
    const [a, b] = [ids.indexOf(from), ids.indexOf(to)].sort((x, y) => x - y);
    if (a < 0) return;
    const next = new Set(base);
    for (const r of shown.slice(a, b + 1)) if (tickable(r)) (on ? next.add(r.id) : next.delete(r.id));
    setSelected(next);
  };
  const tick = (r: ImportRow, shift: boolean) => {
    const on = !selected.has(r.id);
    if (shift && anchor.current != null) tickRange(anchor.current, r.id, on, selected);
    else tickRange(r.id, r.id, on, selected);
    anchor.current = r.id;
  };
  const startSweep = (r: ImportRow, on: boolean) => {
    sweep.current = { from: r.id, on, base: new Set(selected), moved: false };
    const up = () => {
      window.removeEventListener("mouseup", up);
      if (sweep.current?.moved) anchor.current = sweep.current.from;
      sweep.current = null;
    };
    window.addEventListener("mouseup", up);
  };
  const overRow = (r: ImportRow) => {
    const s = sweep.current;
    if (!s || (!s.moved && r.id === s.from)) return;
    s.moved = true;
    tickRange(s.from, r.id, s.on, s.base);
  };
  const shownTickable = shown.filter(tickable);
  const allTicked = shownTickable.length > 0 && shownTickable.every((r) => selected.has(r.id));
  const someTicked = shownTickable.some((r) => selected.has(r.id));
  const tickAll = () => {
    const next = new Set(selected);
    for (const r of shownTickable) (allTicked ? next.delete(r.id) : next.add(r.id));
    setSelected(next);
  };

  const isShot = !!batch && /\.(png|jpe?g)$/i.test(batch.summary.name);
  const categories = [...new Set(rows.flatMap((r) => (r.draft.category ? [r.draft.category] : [])))];
  const locked = !!busy;
  // 多选 · 改分类: the ticked rows that have one, when they are all 收入 or all 支出 (退款 goes with 支出)
  const { data: outKinds = [] } = useKinds(OUT_KINDS);
  const { data: inKinds = [] } = useKinds(IN_KINDS);
  const ticked = rows.filter((r) => selected.has(r.id));
  const flows = new Set(ticked.filter((r) => r.draft.treatment !== "neutral" && !r.draft.refundOf).map((r) => (r.draft.treatment === "in" ? "in" : "out")));
  const flowKinds = flows.size === 1 ? (flows.has("in") ? inKinds : outKinds) : null;
  const setCategory = (category: string | null) =>
    run(async () => {
      if (dirty && !(await leave())) return;
      setEditing(null);
      setDirty(false);
      const n = await importApi.setCategory(ticked.map((r) => r.id), category);
      await refresh();
      toast(`改了 ${n} 笔的分类`);
    });
  return (
    <section aria-label="导入账单" className="absolute inset-0 z-20 flex flex-col gap-3 bg-background px-6 pt-5 pb-4">
      <header className="flex shrink-0 flex-wrap items-center gap-2">
        <h2 className="text-lg font-bold">导入账单</h2>
        <Bird size={14} strokeWidth={1.75} aria-label="柚子的助手 Importer" style={{ color: "var(--module-picky)" }} />
        <span className="mr-auto" />
        {noAi && <span className="text-xs text-muted-foreground">{noAi}</span>}
        <Dropdown ariaLabel="模型" value={model} options={models.length ? models : [{ value: null, label: "没有云端模型" }]} onChange={(m) => m && saveSettings({ model: m })} align="end" />
        <LevelPicker label="思考" model={model} value={settings.data?.level ?? "off"} onChange={(level) => saveSettings({ level })} />
        <Button variant="primary" disabled={locked || !aiOk} onClick={() => void pick()}>
          <FileDown size={14} /> {busy || "导入"}
        </Button>
        <IconButton label="关闭" onClick={() => void leave().then((ok) => ok && props.onClose())}>
          <X size={16} />
        </IconButton>
      </header>
      <Checklist points={points} disabled={locked || !settings.data} onChange={(next) => saveSettings({ points: next })} />
      {(error || job.error || query.error || summaries.error) && (
        <p role="alert" className="shrink-0 text-sm text-destructive">
          {error ?? job.error ?? errorText(query.error ?? summaries.error)}
        </p>
      )}
      <div className="flex min-h-0 flex-1 gap-4">
        <nav aria-label="账单" ref={sort.listRef} className="-m-1 flex w-48 shrink-0 flex-col gap-1 overflow-y-auto p-1">
          {summaries.data?.map((b) => (
            <div key={b.id} {...sort.item(b.id)} className={`group flex items-center gap-1 rounded-control pr-1 ${activeId === b.id ? "bg-accent text-accent-foreground" : "hover:bg-muted"}`}>
              {renaming === b.id ? (
                <div className="min-w-0 flex-1 py-1 pl-1">
                  <NameInput initial={b.title ?? b.name} label="账单名称" max={100} onDone={(t) => rename(b.id, t)} />
                </div>
              ) : (
                <button
                  type="button"
                  disabled={locked}
                  {...(summaries.data.length > 1 && !locked ? sort.handle(b.id) : {})}
                  onClick={() => void leave().then((ok) => ok && show(b.id))}
                  className="min-w-0 flex-1 rounded-control py-2 pl-2.5 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                >
                  <span className="block truncate" title={b.title ? `${b.title}（${b.name}）` : b.name}>
                    {b.title ?? b.name}
                  </span>
                  <span className="text-xs text-muted-foreground">
                    {b.count} 笔{b.pending ? ` · 待核对 ${b.pending}` : ""}
                  </span>
                </button>
              )}
              {renaming !== b.id && (
                <Dropdown<string | null>
                  plain
                  ariaLabel="更多"
                  value={null}
                  align="end"
                  options={[{ value: "rename", label: "改名" }]}
                  actions={[{ label: <span className="text-destructive">删除</span>, onSelect: () => void removeBatch(b) }]}
                  onChange={(v) => v === "rename" && setRenaming(b.id)}
                  renderTrigger={() => <Ellipsis size={14} />}
                  triggerClassName={menuTrigger}
                />
              )}
            </div>
          ))}
        </nav>
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          {!batch ? (
            <p className="py-10 text-center text-muted-foreground">{query.isFetching ? "" : "还没有导入过账单"}</p>
          ) : (
            <>
              <fieldset disabled={locked} className="flex shrink-0 flex-col gap-2 text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  <Dropdown
                    ariaLabel="哪些"
                    value={status}
                    onChange={setStatus}
                    options={[
                      { value: "pending", label: "待核对" },
                      { value: "duplicates", label: "可能重复" },
                      { value: "all", label: "全部" },
                      { value: "ready", label: "已核对" },
                      { value: "posted", label: "已入账" },
                      { value: "duplicate", label: "重复" },
                      { value: "neutral", label: "不计收支" },
                      { value: "ignored", label: "已忽略" },
                    ]}
                  />
                  <DateInput ariaLabel="从" clearable value={filters.from} max={filters.to || undefined} onChange={(from) => setFilters({ ...filters, from })} />
                  <span className="text-muted-foreground">到</span>
                  <DateInput ariaLabel="到" clearable value={filters.to} min={filters.from || undefined} onChange={(to) => setFilters({ ...filters, to })} />
                  <Dropdown ariaLabel="币种" value={filters.currency} options={[{ value: "", label: "全部币种" }, ...CURRENCIES.map((c) => ({ value: c as string, label: CURRENCY_NAME[c] }))]} onChange={(currency) => setFilters({ ...filters, currency })} />
                  <Dropdown ariaLabel="算作" value={filters.treatment} options={[{ value: "", label: "全部收支" }, ...TREATMENTS.map((t) => ({ value: t.value as string, label: t.label }))]} onChange={(treatment) => setFilters({ ...filters, treatment })} />
                  <Dropdown ariaLabel="分类" value={filters.category} options={[{ value: "", label: "全部分类" }, { value: "none", label: UNCATEGORIZED }, ...categories.map((c) => ({ value: c, label: c }))]} onChange={(category) => setFilters({ ...filters, category })} />
                </div>
                <div className="flex items-center gap-2">
                  <TextInput aria-label="搜索" placeholder="搜索" value={filters.text} onChange={(e) => setFilters({ ...filters, text: e.target.value })} className="min-w-0 flex-1" />
                  {filtered && (
                    <Button variant="ghost" onClick={() => (setStatus("pending"), setFilters(NO_FILTERS))}>
                      清除
                    </Button>
                  )}
                  <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
                    {shown.length} / {rows.length}
                  </span>
                </div>
              </fieldset>
              <div className="flex shrink-0 flex-wrap items-center gap-2">
                <Button disabled={locked || dirty} onClick={() => setSelected(new Set(shown.filter(clean).map((r) => r.id)))}>
                  全选没疑点的
                </Button>
                <Button disabled={locked} onClick={() => void add()}>
                  补一笔
                </Button>
                <Button disabled={locked || dirty || !aiOk} onClick={() => void reTidy()}>
                  <Sparkles size={14} /> 再整理
                </Button>
                {isShot && !previews[batch.summary.id] && (
                  <Button disabled={locked} onClick={() => void choosePicture()}>
                    选原图
                  </Button>
                )}
              </div>
              <div className="-m-1 min-h-0 flex-1 overflow-auto p-1">
                {shown.length === 0 ? (
                  <p className="py-8 text-center text-sm text-muted-foreground">没有符合的</p>
                ) : (
                  <table className="w-full overflow-hidden rounded-card border bg-card text-sm select-none">
                    <thead>
                      <tr className="border-b text-left text-xs text-muted-foreground">
                        <th className="w-10 py-2 pl-3 font-normal">
                          <input
                            aria-label="全选"
                            type="checkbox"
                            disabled={locked || !shownTickable.length}
                            checked={allTicked}
                            ref={(el) => {
                              if (el) el.indeterminate = someTicked && !allTicked;
                            }}
                            onChange={tickAll}
                            className="size-4 align-middle accent-[var(--primary)]"
                          />
                        </th>
                        <th className="px-3 py-2 font-normal">日期</th>
                        <th className="px-3 py-2 font-normal">备注</th>
                        <th className="px-3 py-2 font-normal">分类</th>
                        <th className="px-3 py-2 text-right font-normal">金额</th>
                        <th className="px-3 py-2 font-normal">状态</th>
                      </tr>
                    </thead>
                    <tbody>
                      {shown.map((r) => (
                        <tr
                          key={r.id}
                          onMouseDown={(e) => e.button === 0 && !locked && !e.shiftKey && !e.metaKey && tickable(r) && startSweep(r, (e.target as HTMLElement).tagName === "INPUT" ? !selected.has(r.id) : true)}
                          onMouseEnter={() => overRow(r)}
                          onClick={(e) => {
                            if (e.metaKey || e.shiftKey) {
                              if (!locked && tickable(r)) tick(r, e.shiftKey);
                              return;
                            }
                            if (editing !== r.id) void leave().then((ok) => ok && (setEditing(r.id), setDirty(false)));
                          }}
                          className={`cursor-default border-b last:border-b-0 ${editing === r.id ? "bg-accent/60" : selected.has(r.id) ? "bg-primary/5" : "hover:bg-muted/60"}`}
                        >
                          <td className="w-10 py-2 pl-3">
                            <input
                              aria-label="选中"
                              type="checkbox"
                              disabled={locked || !tickable(r)}
                              checked={selected.has(r.id)}
                              onClick={(e) => {
                                e.stopPropagation();
                                tick(r, e.shiftKey);
                              }}
                              onChange={() => {}}
                              className="size-4 align-middle accent-[var(--primary)]"
                            />
                          </td>
                          <td className="px-3 py-2 whitespace-nowrap text-muted-foreground tabular-nums">{r.draft.day ? r.draft.day.slice(5).replace("-", "/") : "—"}</td>
                          <td className="w-full max-w-0 truncate px-3 py-2" title={r.draft.note}>
                            {r.draft.note || <span className="text-muted-foreground">第 {r.source.rowNumber} 行</span>}
                          </td>
                          <td className="px-3 py-2 whitespace-nowrap text-muted-foreground">{r.draft.category ?? ""}</td>
                          <td className={`px-3 py-2 text-right whitespace-nowrap tabular-nums ${r.draft.treatment === "in" ? "text-[var(--status-done-fg)]" : ""}`}>
                            {r.draft.amount == null ? "—" : money(r.draft.treatment === "refund" ? -r.draft.amount : r.draft.amount, r.draft.currency, r.draft.treatment === "in")}
                            {r.draft.treatment === "neutral" && <span className="block text-xs text-muted-foreground">不计收支</span>}
                          </td>
                          <td className="px-3 py-2 text-xs whitespace-nowrap text-muted-foreground">{rowState(r)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </div>
              <footer className="flex shrink-0 items-center justify-end gap-3 text-sm">
                {selected.size > 0 && <span className="text-muted-foreground">选了 {selected.size} 笔</span>}
                {flowKinds && (
                  <Dropdown
                    ariaLabel="改分类"
                    value={"" as string | null}
                    onChange={(c) => void setCategory(c)}
                    options={[...flowKinds.map((k) => ({ value: k.name as string | null, label: k.name })), { value: null as string | null, label: UNCATEGORIZED }]}
                    menuMaxHeight={320}
                    renderTrigger={() => (
                      <>
                        改分类
                        <ChevronDown size={14} className="text-muted-foreground" />
                      </>
                    )}
                  />
                )}
                <Button variant="primary" disabled={locked || dirty || selected.size === 0} onClick={() => void post()}>
                  入账
                </Button>
              </footer>
            </>
          )}
        </div>
        {row && !busy && (
          <ReviewRow
            key={`${row.id}:${row.state}:${row.resolution}:${row.draft.category}:${row.draft.note}`}
            row={row}
            preview={activeId ? previews[activeId] : undefined}
            dirty={dirty}
            onDirty={setDirty}
            onSaved={() => {
              setSelected(new Set());
              void refresh();
            }}
            onClose={() => void leave().then((ok) => ok && (setEditing(null), setDirty(false)))}
          />
        )}
      </div>
      {confirmDialog}
    </section>
  );
}
