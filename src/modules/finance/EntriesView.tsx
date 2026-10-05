// 收支: a month's entries (filter by 支出 / 收入, 分类, 账户) and the 固定支出 list.
import { useMemo, useState, type ReactNode } from "react";
import { ChevronLeft, ChevronRight, Plus, Repeat, Rows3 } from "lucide-react";
import { localToday, weekday } from "@/shared/dates";
import { Dropdown } from "@/shared/Dropdown";
import { KindManager } from "@/shared/kinds/KindManager";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { useSortable } from "@/shared/sortable";
import { Button, IconButton, Segmented } from "@/shared/ui";
import { FIN_KEY, IN_KINDS, OUT_KINDS, useAccounts, useEntries, useFinMutations, useFixed, type Entry, type Flow } from "./api";
import { EntryPanel, FixedPanel } from "./EntryPanel";
import { CURRENCIES, money, monthTitle, ruleText, shiftMonth, thisMonth, type Currency } from "./money";
import { WeekDialog } from "./WeekDialog";

type Tab = "list" | "fixed";
const shortDay = (d: string) => `${Number(d.slice(5, 7))}/${Number(d.slice(8))} ${weekday(d)}`;
const slash = (d: string) => d.split("-").join(" / ");

/** "S$1,234 · ¥560" — each currency on its own, never added up. */
function sums(entries: Entry[], flow: Flow) {
  return CURRENCIES.map((c) => [c, entries.filter((e) => e.flow === flow && e.currency === c).reduce((s, e) => s + e.amount, 0)] as const)
    .filter(([, n]) => n > 0)
    .map(([c, n]) => money(n, c))
    .join(" · ");
}

function Th(props: { children?: ReactNode; right?: boolean; className?: string }) {
  return <th className={`px-3 py-2 font-normal ${props.right ? "text-right" : "text-left"} ${props.className ?? ""}`}>{props.children}</th>;
}

export function EntriesView() {
  const [month, setMonth] = useState(thisMonth());
  const [tab, setTab] = useState<Tab>("list");
  const [flow, setFlow] = useState<"all" | Flow>("all");
  const [kindFilter, setKindFilter] = useState<number | "all" | null>("all");
  const [accountFilter, setAccountFilter] = useState<number | "all" | null>("all");
  const [week, setWeek] = useState(false);
  const [managing, setManaging] = useState<Flow | null>(null);
  const { data: entries = [] } = useEntries(month);
  const { data: fixed = [] } = useFixed();
  const { data: accounts = [] } = useAccounts();
  const { data: outKinds = [] } = useKinds(OUT_KINDS);
  const { data: inKinds = [] } = useKinds(IN_KINDS);
  const m = useFinMutations();
  const panel = usePanelSelection<string>();
  const sortFixed = useSortable<HTMLTableSectionElement>((ids) => m.reorderFixed.mutate(ids));

  const kindName = new Map([...outKinds, ...inKinds].map((k) => [k.id, k.name]));
  const accountName = new Map(accounts.map((a) => [a.id, a.name]));
  // default currency for new entries: this month's most used, else the first account's
  const defaultCurrency = useMemo<Currency>(() => {
    const n = new Map<Currency, number>();
    for (const e of entries) n.set(e.currency, (n.get(e.currency) ?? 0) + 1);
    const top = [...n.entries()].sort((a, b) => b[1] - a[1])[0];
    return top?.[0] ?? accounts.find((a) => !a.closed)?.currency ?? "CNY";
  }, [entries, accounts]);

  const shown = entries.filter(
    (e) => (flow === "all" || e.flow === flow) && (kindFilter === "all" || e.kindId === kindFilter) && (accountFilter === "all" || e.accountId === accountFilter),
  );
  const editing = panel.selection?.startsWith("e") ? entries.find((e) => `e${e.id}` === panel.selection) : undefined;
  const editingFixed = panel.selection?.startsWith("f") ? fixed.find((f) => `f${f.id}` === panel.selection) : undefined;
  const isCurrent = month === thisMonth();
  const defaultDay = isCurrent ? localToday() : `${month}-01`;

  usePageKeys({
    onNew: () => void panel.request(tab === "list" ? "new" : "fnew"),
    onEscape: panel.selection !== null ? () => void panel.request(null) : undefined,
  });

  const kindOptions = [
    { value: "all" as const, label: "全部分类" },
    ...(flow !== "in" ? outKinds : []).map((k) => ({ value: k.id, label: k.name })),
    ...(flow !== "out" ? inKinds : []).map((k) => ({ value: k.id, label: flow === "all" ? `${k.name}（收入）` : k.name })),
    { value: null, label: UNCATEGORIZED },
  ];
  const out = sums(entries, "out");
  const income = sums(entries, "in");

  return (
    <>
      <div className="flex shrink-0 items-start justify-between gap-4 pb-3">
        <div className="min-w-0">
          <div className="flex items-center gap-1">
            <h2 className="text-lg font-bold">{monthTitle(month)}</h2>
            <IconButton label="上个月" onClick={() => setMonth((x) => shiftMonth(x, -1))}>
              <ChevronLeft size={16} />
            </IconButton>
            <IconButton label="下个月" onClick={() => setMonth((x) => shiftMonth(x, 1))}>
              <ChevronRight size={16} />
            </IconButton>
            {!isCurrent && (
              <Button variant="ghost" onClick={() => setMonth(thisMonth())}>
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
              <Button onClick={() => setWeek(true)}>
                <Rows3 size={14} /> 记一周小额
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
          onChange={setTab}
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
            <Dropdown ariaLabel="分类" value={kindFilter} onChange={setKindFilter} options={kindOptions} menuMaxHeight={320} />
            <Dropdown
              ariaLabel="账户"
              value={accountFilter}
              onChange={setAccountFilter}
              options={[{ value: "all" as const, label: "全部账户" }, ...accounts.map((a) => ({ value: a.id, label: a.name })), { value: null, label: "没填账户" }]}
              menuMaxHeight={320}
            />
          </>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto pb-4">
        {tab === "list" ? (
          shown.length === 0 ? (
            <p className="py-10 text-center text-muted-foreground">{entries.length === 0 ? "这个月还没有记录" : "没有符合的记录"}</p>
          ) : (
            <table className="w-full overflow-hidden rounded-card border bg-card text-sm">
              <thead className="border-b text-xs text-muted-foreground">
                <tr>
                  <Th className="w-24">日期</Th>
                  <Th className="w-24">分类</Th>
                  <Th>备注</Th>
                  <Th className="w-32">账户</Th>
                  <Th right className="w-32">
                    金额
                  </Th>
                </tr>
              </thead>
              <tbody>
                {shown.map((e) => (
                  <tr
                    key={e.id}
                    onClick={() => void panel.request(`e${e.id}`)}
                    className={`cursor-pointer border-b transition-colors duration-150 last:border-b-0 ${panel.selection === `e${e.id}` ? "bg-accent" : "hover:bg-muted/70"}`}
                  >
                    <td className="px-3 py-2 text-muted-foreground tabular-nums">{shortDay(e.day)}</td>
                    <td className="px-3 py-2">{e.kindId !== null ? kindName.get(e.kindId) : <span className="text-muted-foreground">{UNCATEGORIZED}</span>}</td>
                    <td className="max-w-0 truncate px-3 py-2">
                      {e.fixedId !== null && <Repeat size={12} className="mr-1 inline text-muted-foreground" aria-label="固定支出" />}
                      {e.note}
                    </td>
                    <td className="truncate px-3 py-2 text-muted-foreground">{e.accountId !== null ? accountName.get(e.accountId) : ""}</td>
                    <td className={`px-3 py-2 text-right tabular-nums ${e.flow === "in" ? "text-[var(--status-done-fg)]" : ""}`}>{money(e.amount, e.currency, e.flow === "in")}</td>
                  </tr>
                ))}
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
                <Th className="w-28">账户</Th>
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
                  <td className="truncate px-3 py-2 text-muted-foreground">{f.accountId !== null ? accountName.get(f.accountId) : ""}</td>
                  <td className="px-3 py-2">{ruleText(f.rule)}</td>
                  <td className="px-3 py-2 tabular-nums">{f.stopped ? "—" : slash(f.nextDay)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

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
      {week && <WeekDialog defaultCurrency={defaultCurrency} onClose={() => setWeek(false)} />}
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
    </>
  );
}
