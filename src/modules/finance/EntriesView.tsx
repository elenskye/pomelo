// 收支: a month's entries (filter by 支出 / 收入 and 分类; change a row's 分类 in place; 多选 for batch
// 分类 / 币种 / 删除) and the 固定支出 list. Periods from 记小额 show in every month they cover.
import { useEffect, useMemo, useState, type MouseEvent, type ReactNode } from "react";
import { ChevronDown, ChevronLeft, ChevronRight, ListChecks, Plus, Repeat, Rows3, Trash2 } from "lucide-react";
import { localToday, weekday } from "@/shared/dates";
import { Dropdown } from "@/shared/Dropdown";
import { useConfirm, useToast, errorText } from "@/shared/feedback";
import { KindManager } from "@/shared/kinds/KindManager";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { useSortable } from "@/shared/sortable";
import { Button, IconButton, Segmented } from "@/shared/ui";
import { FIN_KEY, IN_KINDS, OUT_KINDS, useAccounts, useEntries, useFinMutations, useFixed, useTotals, type Entry, type Flow, type Total } from "./api";
import { EntryPanel, FixedPanel } from "./EntryPanel";
import { CURRENCIES, CURRENCY_NAME, money, monthTitle, ruleText, shiftMonth, thisMonth, type Currency } from "./money";
import { SmallDialog } from "./SmallDialog";
import type { Hit } from "@/shared/open";
import { usePageContext } from "@/app/pageContext";

type Tab = "list" | "fixed";
const md = (d: string) => `${Number(d.slice(5, 7))}/${Number(d.slice(8))}`;
const when = (e: Entry) => (e.endDay ? `${md(e.day)} – ${md(e.endDay)}` : `${md(e.day)} ${weekday(e.day)}`);
const slash = (d: string) => d.split("-").join(" / ");

/** "S$1,234 · ¥560" — each currency on its own, never added up (a period counts with this month's share). */
function sums(totals: Total[], flow: Flow) {
  return CURRENCIES.map((c) => [c, totals.filter((t) => t.flow === flow && t.currency === c).reduce((s, t) => s + t.amount, 0)] as const)
    .filter(([, n]) => n > 0)
    .map(([c, n]) => money(n, c))
    .join(" · ");
}

function Th(props: { children?: ReactNode; right?: boolean; className?: string }) {
  return <th className={`px-3 py-2 font-normal ${props.right ? "text-right" : "text-left"} ${props.className ?? ""}`}>{props.children}</th>;
}

const check = "pointer-events-none size-4 align-middle accent-[var(--primary)]";

/** Bordered trigger with a fixed label (the menu picks a value to apply, nothing stays selected). */
const actionTrigger = (label: string) => () => (
  <>
    {label}
    <ChevronDown size={14} className="text-muted-foreground" />
  </>
);

export function EntriesView(props: { focus?: Hit | null }) {
  const [month, setMonth] = useState(thisMonth());
  const [tab, setTab] = useState<Tab>("list");
  const [flow, setFlow] = useState<"all" | Flow>("all");
  const [kindFilter, setKindFilter] = useState<number | "all" | null>("all");
  const [small, setSmall] = useState(false);
  const [managing, setManaging] = useState<Flow | null>(null);
  const [selecting, setSelecting] = useState(false);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const { data: entries = [] } = useEntries(month);
  const { data: totals = [] } = useTotals();
  const { data: fixed = [] } = useFixed();
  const { data: accounts = [] } = useAccounts();
  const { data: outKinds = [] } = useKinds(OUT_KINDS);
  const { data: inKinds = [] } = useKinds(IN_KINDS);
  const m = useFinMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const panel = usePanelSelection<string>();
  const sortFixed = useSortable<HTMLTableSectionElement>((ids) => m.reorderFixed.mutate(ids));

  const kindName = new Map([...outKinds, ...inKinds].map((k) => [k.id, k.name]));
  const kindsOf = (f: Flow) => (f === "out" ? outKinds : inKinds);
  // default currency for new entries: this month's most used, else the first account's
  const defaultCurrency = useMemo<Currency>(() => {
    const n = new Map<Currency, number>();
    for (const e of entries) n.set(e.currency, (n.get(e.currency) ?? 0) + 1);
    const top = [...n.entries()].sort((a, b) => b[1] - a[1])[0];
    return top?.[0] ?? accounts.find((a) => !a.closed)?.currency ?? "CNY";
  }, [entries, accounts]);

  const shown = entries.filter((e) => (flow === "all" || e.flow === flow) && (kindFilter === "all" || e.kindId === kindFilter));
  const chosen = shown.filter((e) => picked.has(e.id));
  const chosenFlows = new Set(chosen.map((e) => e.flow));
  const editing = panel.selection?.startsWith("e") ? entries.find((e) => `e${e.id}` === panel.selection) : undefined;
  const editingFixed = panel.selection?.startsWith("f") ? fixed.find((f) => `f${f.id}` === panel.selection) : undefined;
  const isCurrent = month === thisMonth();
  const defaultDay = isCurrent ? localToday() : `${month}-01`;
  usePageContext({
    module: "finance",
    page: tab === "list" ? "收支" : "固定支出",
    filter:
      tab === "list"
        ? [`${month.slice(0, 4)} 年 ${Number(month.slice(5))} 月`, flow === "all" ? null : flow === "out" ? "支出" : "收入", kindFilter === "all" ? null : kindFilter === null ? "未分类" : (kindName.get(kindFilter) ?? null)].filter(Boolean).join(" · ")
        : null,
    record: editing
      ? { kind: "entry", id: editing.id, label: `收支：${editing.day.slice(5).replace("-", "/")} ${kindName.get(editing.kindId ?? -1) ?? "未分类"}${editing.note ? ` · ${editing.note}` : ""}` }
      : editingFixed
        ? { kind: "fixed", id: editingFixed.id, label: `固定支出：${editingFixed.name}` }
        : null,
    selected: selecting && chosen.length ? { kind: "entry", ids: chosen.map((e) => e.id), label: `收支：选中的 ${chosen.length} 笔` } : null,
  });

  const stopSelecting = () => {
    setSelecting(false);
    setPicked(new Set());
    setAnchor(null);
  };
  const goMonth = (next: string) => {
    stopSelecting();
    setMonth(next);
  };

  // 全局搜索: an entry (its month, 明细) or a 固定支出
  useEffect(() => {
    const h = props.focus;
    if (!h || (h.kind !== "entry" && h.kind !== "fixed")) return;
    stopSelecting();
    if (h.kind === "entry") {
      setTab("list");
      setFlow("all");
      setKindFilter("all");
      if (h.day) setMonth(h.day.slice(0, 7));
      void panel.request(`e${h.id}`);
    } else {
      setTab("fixed");
      void panel.request(`f${h.id}`);
    }
  }, [props.focus]); // eslint-disable-line react-hooks/exhaustive-deps

  usePageKeys({
    onNew: () => void panel.request(tab === "list" ? "new" : "fnew"),
    onEscape: selecting ? stopSelecting : panel.selection !== null ? () => void panel.request(null) : undefined,
  });

  /** Plain click opens the entry; ⌘-click toggles it into the selection, ⇧-click selects a range (both start 多选). */
  const clickRow = async (ev: MouseEvent, e: Entry) => {
    const add = ev.metaKey || ev.ctrlKey;
    const range = ev.shiftKey;
    if (!selecting && !add && !range) return void panel.request(`e${e.id}`);
    let base = picked;
    let from = anchor;
    if (!selecting) {
      if (!(await panel.request(null))) return;
      base = new Set(editing ? [editing.id] : []);
      from = editing?.id ?? null;
      setSelecting(true);
    }
    const next = new Set(base);
    const [i, j] = [shown.findIndex((x) => x.id === from), shown.findIndex((x) => x.id === e.id)];
    if (range && i >= 0) shown.slice(Math.min(i, j), Math.max(i, j) + 1).forEach((x) => next.add(x.id));
    else if (next.has(e.id)) next.delete(e.id);
    else next.add(e.id);
    setPicked(next);
    setAnchor(e.id);
  };

  const ids = chosen.map((e) => e.id);
  const batchKind = (kindId: number | null) =>
    m.updateEntries.mutate({ ids, patch: { kindId } }, { onSuccess: () => toast(`已把 ${ids.length} 笔改成「${kindId === null ? UNCATEGORIZED : kindName.get(kindId)}」`) });
  const batchCurrency = (currency: Currency) =>
    m.updateEntries.mutate({ ids, patch: { currency } }, { onSuccess: () => toast(`已把 ${ids.length} 笔改成${CURRENCY_NAME[currency]}`) });
  const batchDelete = async () => {
    if (!(await confirm({ title: `删除选中的 ${ids.length} 笔？`, confirmLabel: "删除", danger: true }))) return;
    m.deleteEntries.mutate(ids, {
      onSuccess: () => {
        toast(`已删除 ${ids.length} 笔`);
        stopSelecting();
      },
    });
  };
  const setKind = (e: Entry, kindId: number | null) =>
    m.updateEntry.mutate({ id: e.id, patch: { kindId } }, { onError: (err) => toast(errorText(err), "error") });

  const kindOptions = [
    { value: "all" as const, label: "全部分类" },
    ...(flow !== "in" ? outKinds : []).map((k) => ({ value: k.id, label: k.name })),
    ...(flow !== "out" ? inKinds : []).map((k) => ({ value: k.id, label: flow === "all" ? `${k.name}（收入）` : k.name })),
    { value: null, label: UNCATEGORIZED },
  ];
  const manageActions = [
    ...(flow !== "in" ? [{ label: "管理支出分类…", onSelect: () => setManaging("out") }] : []),
    ...(flow !== "out" ? [{ label: "管理收入分类…", onSelect: () => setManaging("in") }] : []),
  ];
  const monthTotals = totals.filter((t) => t.month === month);
  const out = sums(monthTotals, "out");
  const income = sums(monthTotals, "in");
  const allPicked = shown.length > 0 && chosen.length === shown.length;

  return (
    <>
      <div className="flex shrink-0 items-start justify-between gap-4 pb-3">
        <div className="min-w-0">
          <div className="flex items-center gap-1">
            <h2 className="text-lg font-bold">{monthTitle(month)}</h2>
            <IconButton label="上个月" onClick={() => goMonth(shiftMonth(month, -1))}>
              <ChevronLeft size={16} />
            </IconButton>
            <IconButton label="下个月" onClick={() => goMonth(shiftMonth(month, 1))}>
              <ChevronRight size={16} />
            </IconButton>
            {!isCurrent && (
              <Button variant="ghost" onClick={() => goMonth(thisMonth())}>
                本月
              </Button>
            )}
          </div>
          <p className="text-sm text-muted-foreground tabular-nums">
            支出 {out || "—"} ｜ 收入 {income || "—"}
          </p>
        </div>
        <div className="flex shrink-0 gap-2">
          {tab === "list" ? (
            <>
              <Button onClick={() => setSmall(true)}>
                <Rows3 size={14} /> 记小额
              </Button>
              <Button variant="primary" onClick={() => void panel.request("new")}>
                <Plus size={14} /> 记一笔
              </Button>
            </>
          ) : (
            <Button variant="primary" onClick={() => void panel.request("fnew")}>
              <Plus size={14} /> 新固定支出
            </Button>
          )}
        </div>
      </div>
      <div className="flex shrink-0 flex-wrap items-center gap-2 pb-3">
        <Segmented
          ariaLabel="明细还是固定支出"
          value={tab}
          onChange={(t) => {
            stopSelecting();
            setTab(t);
          }}
          options={[
            { value: "list" as Tab, label: "明细" },
            { value: "fixed" as Tab, label: `固定支出 ${fixed.filter((f) => !f.stopped).length}` },
          ]}
        />
        {tab === "list" && (
          <>
            <div className="mx-1 h-5 w-px bg-border" />
            <Segmented
              ariaLabel="支出还是收入"
              value={flow}
              onChange={(v) => {
                setFlow(v);
                setKindFilter("all");
              }}
              options={[
                { value: "all" as const, label: "全部" },
                { value: "out" as const, label: "支出" },
                { value: "in" as const, label: "收入" },
              ]}
            />
            <Dropdown ariaLabel="分类" value={kindFilter} onChange={setKindFilter} options={kindOptions} actions={manageActions} menuMaxHeight={320} />
            <Button variant={selecting ? "primary" : "ghost"} className="ml-auto" onClick={() => (selecting ? stopSelecting() : setSelecting(true))} aria-pressed={selecting}>
              <ListChecks size={14} /> {selecting ? "完成" : "多选"}
            </Button>
          </>
        )}
      </div>

      <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1 pb-4">
        {tab === "list" ? (
          shown.length === 0 ? (
            <p className="py-10 text-center text-muted-foreground">{entries.length === 0 ? "这个月还没有记录" : "没有符合的记录"}</p>
          ) : (
            <table className="w-full overflow-hidden rounded-card border bg-card text-sm">
              <thead className="border-b text-xs text-muted-foreground">
                <tr>
                  {selecting && (
                    <th className="w-10 py-2 pl-3 text-left font-normal">
                      <input
                        type="checkbox"
                        aria-label="全选"
                        checked={allPicked}
                        ref={(el) => {
                          if (el) el.indeterminate = chosen.length > 0 && !allPicked;
                        }}
                        onChange={() => setPicked(allPicked ? new Set() : new Set(shown.map((e) => e.id)))}
                        className="size-4 align-middle accent-[var(--primary)]"
                      />
                    </th>
                  )}
                  <Th className="w-32">日期</Th>
                  <Th className="w-28">分类</Th>
                  <Th>备注</Th>
                  <Th right className="w-32">
                    金额
                  </Th>
                </tr>
              </thead>
              <tbody>
                {shown.map((e) => {
                  const on = selecting ? picked.has(e.id) : panel.selection === `e${e.id}`;
                  const name = e.kindId !== null ? kindName.get(e.kindId) : undefined;
                  return (
                    <tr
                      key={e.id}
                      onClick={(ev) => void clickRow(ev, e)}
                      onMouseDown={(ev) => ev.shiftKey && ev.preventDefault()} // no text selection on ⇧-click
                      className={`cursor-pointer border-b transition-colors duration-150 last:border-b-0 ${on ? "bg-accent" : "hover:bg-muted/70"}`}
                    >
                      {selecting && (
                        <td className="py-2 pl-3">
                          <input type="checkbox" readOnly tabIndex={-1} aria-label="选中" checked={picked.has(e.id)} className={check} />
                        </td>
                      )}
                      <td className="px-3 py-2 whitespace-nowrap text-muted-foreground tabular-nums">{when(e)}</td>
                      <td className="px-3 py-1.5">
                        {selecting ? (
                          (name ?? <span className="text-muted-foreground">{UNCATEGORIZED}</span>)
                        ) : (
                          <Dropdown
                            ariaLabel="分类"
                            value={e.kindId}
                            onChange={(v) => setKind(e, v)}
                            options={[...kindsOf(e.flow).map((k) => ({ value: k.id as number | null, label: k.name })), { value: null, label: UNCATEGORIZED }]}
                            actions={[{ label: "管理分类…", onSelect: () => setManaging(e.flow) }]}
                            menuMaxHeight={320}
                            triggerClassName="group/kind -mx-1.5 inline-flex max-w-full items-center gap-1 rounded-control px-1.5 py-0.5 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
                            renderTrigger={(_, open) => (
                              <>
                                <span className={`truncate ${name ? "" : "text-muted-foreground"}`}>{name ?? UNCATEGORIZED}</span>
                                <ChevronDown size={12} className={`shrink-0 text-muted-foreground ${open ? "" : "opacity-0 group-hover/kind:opacity-100 group-focus-visible/kind:opacity-100"}`} />
                              </>
                            )}
                          />
                        )}
                      </td>
                      <td className="max-w-0 truncate px-3 py-2">
                        {e.fixedId !== null && <Repeat size={12} className="mr-1 inline text-muted-foreground" aria-label="固定支出" />}
                        {e.note}
                      </td>
                      <td className={`px-3 py-2 text-right tabular-nums ${e.flow === "in" ? "text-[var(--status-done-fg)]" : ""}`}>{money(e.amount, e.currency, e.flow === "in")}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )
        ) : fixed.length === 0 ? (
          <p className="py-10 text-center text-muted-foreground">还没有固定支出</p>
        ) : (
          <table className="w-full overflow-hidden rounded-card border bg-card text-sm">
            <thead className="border-b text-xs text-muted-foreground">
              <tr>
                <Th>名称</Th>
                <Th right className="w-32">
                  金额
                </Th>
                <Th className="w-24">分类</Th>
                <Th className="w-32">周期</Th>
                <Th className="w-32">下次</Th>
              </tr>
            </thead>
            <tbody ref={sortFixed.listRef}>
              {fixed.map((f) => (
                <tr
                  key={f.id}
                  {...sortFixed.item(f.id)}
                  {...(fixed.length > 1 ? sortFixed.handle(f.id) : {})}
                  onClick={() => void panel.request(`f${f.id}`)}
                  className={`cursor-pointer border-b bg-card transition-colors duration-150 last:border-b-0 ${f.stopped ? "text-muted-foreground" : ""} ${
                    panel.selection === `f${f.id}` ? "bg-accent" : "hover:bg-muted/70"
                  }`}
                >
                  <td className="px-3 py-2">
                    {f.name}
                    {f.stopped && <span className="ml-2 rounded bg-muted px-1.5 text-xs">停用</span>}
                  </td>
                  <td className={`px-3 py-2 text-right tabular-nums ${f.flow === "in" && !f.stopped ? "text-[var(--status-done-fg)]" : ""}`}>{money(f.amount, f.currency, f.flow === "in")}</td>
                  <td className="px-3 py-2">{f.kindId !== null ? kindName.get(f.kindId) : ""}</td>
                  <td className="px-3 py-2">{ruleText(f.rule)}</td>
                  <td className="px-3 py-2 tabular-nums">{f.stopped ? "—" : slash(f.nextDay)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {tab === "list" && selecting && (
        <div className="flex shrink-0 items-center gap-2 border-t pt-3">
          <span className="mr-auto text-sm text-muted-foreground tabular-nums">已选 {chosen.length} 笔</span>
          {chosenFlows.size === 1 && (
            <Dropdown
              ariaLabel="改分类"
              value={-1 as number | null}
              onChange={batchKind}
              options={[...kindsOf([...chosenFlows][0]).map((k) => ({ value: k.id as number | null, label: k.name })), { value: null, label: UNCATEGORIZED }]}
              menuMaxHeight={320}
              renderTrigger={actionTrigger("改分类")}
            />
          )}
          {chosen.length > 0 && (
            <>
              <Dropdown
                ariaLabel="改币种"
                value={"" as Currency | ""}
                onChange={(c) => c && batchCurrency(c)}
                options={CURRENCIES.map((c) => ({ value: c as Currency | "", label: CURRENCY_NAME[c] }))}
                renderTrigger={actionTrigger("改币种")}
              />
              <Button variant="ghost-danger" onClick={() => void batchDelete()}>
                <Trash2 size={14} /> 删除
              </Button>
            </>
          )}
        </div>
      )}

      {(panel.selection === "new" || editing) && (
        <EntryPanel
          key={panel.selection}
          entry={editing ?? null}
          defaultDay={defaultDay}
          defaultCurrency={defaultCurrency}
          onClose={() => void panel.request(null)}
          onDone={panel.done}
          onDirtyChange={panel.setDirty}
          onManageKinds={setManaging}
        />
      )}
      {(panel.selection === "fnew" || editingFixed) && (
        <FixedPanel
          key={panel.selection}
          item={editingFixed ?? null}
          defaultCurrency={defaultCurrency}
          onClose={() => void panel.request(null)}
          onDone={panel.done}
          onDirtyChange={panel.setDirty}
          onManageKinds={setManaging}
        />
      )}
      {small && <SmallDialog defaultCurrency={defaultCurrency} onClose={() => setSmall(false)} />}
      {managing && (
        <KindManager
          module={managing === "out" ? OUT_KINDS : IN_KINDS}
          moduleLabel={managing === "out" ? "支出" : "收入"}
          unit="笔"
          noun="分类"
          itemsKey={FIN_KEY}
          onClose={() => setManaging(null)}
        />
      )}
      {panel.confirmDialog}
      {confirmDialog}
    </>
  );
}
