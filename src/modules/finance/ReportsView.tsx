// 报表, one currency at a time (amounts are never converted here):
//   月 — 支出 / 收入 / 结余, 分类占比 (donut + ranked list), 预算 vs spent
//   年 — the same numbers for the year, 每月趋势 (支出 vs 收入 columns), 年度分类
// Category colors follow the category, not this period's rank: the 4 biggest spenders of all time
// (in this currency) get --chart-3…6, everything else is 其他. --chart-1 / --chart-2 are 收入 / 支出.
import { useState, type FormEvent, type ReactNode } from "react";
import { ChevronLeft, ChevronRight, Target } from "lucide-react";
import { BarList, ColumnChart, Donut, Meter } from "@/shared/charts";
import { errorText, useToast } from "@/shared/feedback";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { Modal } from "@/shared/Modal";
import { Button, IconButton, Segmented } from "@/shared/ui";
import { OUT_KINDS, useBudgets, useFinMutations, useTotals, type Total } from "./api";
import { AmountInput, CurrencyPicker } from "./fields";
import { CURRENCIES, money, moneyInput, moneyShort, monthTitle, parseMoney, shiftMonth, thisMonth, type Currency } from "./money";

type Mode = "month" | "year";
const SLOTS = ["var(--chart-3)", "var(--chart-4)", "var(--chart-5)", "var(--chart-6)"];
const OTHER = "var(--chart-other)";

function Kpi(props: { label: string; value: string; tone?: "good" | "bad" }) {
  return (
    <div className="rounded-card border bg-card px-4 py-3">
      <p className="text-xs text-muted-foreground">{props.label}</p>
      <p className={`text-xl font-bold ${props.tone === "good" ? "text-[var(--status-done-fg)]" : props.tone === "bad" ? "text-destructive" : ""}`}>{props.value}</p>
    </div>
  );
}

function Card(props: { title: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="rounded-card border bg-card px-4 py-3">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm text-muted-foreground">{props.title}</h3>
        {props.actions}
      </div>
      {props.children}
    </section>
  );
}

function BudgetDialog(props: { currency: Currency; onClose: () => void }) {
  const { data: kinds = [] } = useKinds(OUT_KINDS);
  const { data: budgets = [] } = useBudgets();
  const [currency, setCurrency] = useState(props.currency);
  const [edits, setEdits] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const m = useFinMutations();
  const toast = useToast();
  const key = (kindId: number) => `${currency}:${kindId}`;
  const value = (kindId: number) => edits[key(kindId)] ?? moneyInput(budgets.find((b) => b.kindId === kindId && b.currency === currency)?.amount);

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const rows = kinds.map((k) => ({ k, text: value(k.id).trim() }));
    const bad = rows.find((r) => r.text && !((parseMoney(r.text) ?? 0) > 0));
    if (bad) return setError(`「${bad.k.name}」的预算不对`);
    try {
      await m.saveBudgets.mutateAsync({ currency, rows: rows.map((r) => ({ kindId: r.k.id, amount: r.text ? parseMoney(r.text) : null })) });
      toast("已保存预算");
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Modal title="每月预算" onClose={props.onClose} onSubmit={save} error={error} submitLabel="保存" wide>
      <CurrencyPicker value={currency} onChange={setCurrency} />
      <div className="grid max-h-[50vh] grid-cols-2 gap-x-4 gap-y-2 overflow-y-auto pr-1">
        {kinds.map((k) => (
          <label key={k.id} className="flex items-center gap-2 text-sm">
            <span className="w-16 shrink-0 truncate">{k.name}</span>
            <AmountInput label={`${k.name}的预算`} value={value(k.id)} onChange={(v) => setEdits((x) => ({ ...x, [key(k.id)]: v }))} />
          </label>
        ))}
      </div>
    </Modal>
  );
}

export function ReportsView() {
  const { data: totals = [] } = useTotals();
  const { data: budgets = [] } = useBudgets();
  const { data: kinds = [] } = useKinds(OUT_KINDS);
  const [mode, setMode] = useState<Mode>("month");
  const [month, setMonth] = useState(thisMonth());
  const [year, setYear] = useState(Number(thisMonth().slice(0, 4)));
  const [picked, setPicked] = useState<Currency | null>(null);
  const [budgetDialog, setBudgetDialog] = useState(false);

  const inPeriod = (t: Total) => (mode === "month" ? t.month === month : t.month.startsWith(`${year}-`));
  const periodRows = totals.filter(inPeriod);
  // currency: the one picked, else this period's most used, else any with data
  const used = CURRENCIES.filter((c) => totals.some((t) => t.currency === c));
  const busiest = CURRENCIES.map((c) => [c, periodRows.filter((t) => t.currency === c).reduce((s, t) => s + t.count, 0)] as const).sort((a, b) => b[1] - a[1])[0];
  const currency: Currency = picked ?? (busiest && busiest[1] > 0 ? busiest[0] : (used[0] ?? "CNY"));
  const rows = periodRows.filter((t) => t.currency === currency);
  const sum = (flow: "out" | "in", rs = rows) => rs.filter((t) => t.flow === flow).reduce((s, t) => s + t.amount, 0);
  const fmt = (n: number) => money(n, currency);
  const kindName = new Map(kinds.map((k) => [k.id, k.name]));

  // stable colors: top 4 spending categories of all time in this currency
  const allTime = new Map<number, number>();
  for (const t of totals) if (t.currency === currency && t.flow === "out" && t.kindId !== null) allTime.set(t.kindId, (allTime.get(t.kindId) ?? 0) + t.amount);
  const colorOf = new Map([...allTime.entries()].sort((a, b) => b[1] - a[1]).slice(0, SLOTS.length).map(([id], i) => [id, SLOTS[i]]));

  const byKind = new Map<number | null, number>();
  for (const t of rows.filter((t) => t.flow === "out")) byKind.set(t.kindId, (byKind.get(t.kindId) ?? 0) + t.amount);
  const ranked = [...byKind.entries()].sort((a, b) => b[1] - a[1]);
  const spent = sum("out");
  const named = ranked.filter(([id]) => id !== null && colorOf.has(id));
  const slices = [
    ...named.map(([id, v]) => ({ key: String(id), label: kindName.get(id!) ?? UNCATEGORIZED, value: v, color: colorOf.get(id!)! })),
    ...(spent - named.reduce((s, [, v]) => s + v, 0) > 0 ? [{ key: "other", label: "其他", value: spent - named.reduce((s, [, v]) => s + v, 0), color: OTHER }] : []),
  ];
  const share = (
    <Card title={mode === "month" ? "分类占比" : "年度分类"}>
      {spent === 0 ? (
        <p className="py-6 text-center text-sm text-muted-foreground">没有支出</p>
      ) : (
        <div className="flex items-start gap-6">
          <Donut
            slices={slices}
            format={fmt}
            center={
              <>
                <span className="text-xs text-muted-foreground">支出</span>
                <span className="text-base font-bold">{fmt(spent)}</span>
              </>
            }
          />
          <div className="min-w-0 flex-1">
            <BarList
              rows={ranked.map(([id, v]) => ({
                key: String(id),
                label: id === null ? UNCATEGORIZED : (kindName.get(id) ?? UNCATEGORIZED),
                value: v,
                color: id !== null && colorOf.has(id) ? colorOf.get(id)! : OTHER,
                detail: `${Math.round((v / spent) * 100)}%`,
              }))}
              format={fmt}
            />
          </div>
        </div>
      )}
    </Card>
  );

  const income = sum("in");
  const balance = income - spent;
  const months = Array.from({ length: 12 }, (_, i) => `${year}-${String(i + 1).padStart(2, "0")}`);
  const monthsElapsed = year < Number(thisMonth().slice(0, 4)) ? 12 : year > Number(thisMonth().slice(0, 4)) ? 0 : Number(thisMonth().slice(5));
  const myBudgets = budgets.filter((b) => b.currency === currency);

  return (
    <>
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 pb-3">
        <div className="flex items-center gap-1">
          <h2 className="text-lg font-bold">{mode === "month" ? monthTitle(month) : `${year} 年`}</h2>
          <IconButton label="往前" onClick={() => (mode === "month" ? setMonth((x) => shiftMonth(x, -1)) : setYear((y) => y - 1))}>
            <ChevronLeft size={16} />
          </IconButton>
          <IconButton label="往后" onClick={() => (mode === "month" ? setMonth((x) => shiftMonth(x, 1)) : setYear((y) => y + 1))}>
            <ChevronRight size={16} />
          </IconButton>
        </div>
        <div className="flex items-center gap-2">
          <Segmented
            ariaLabel="按月还是按年"
            value={mode}
            onChange={setMode}
            options={[
              { value: "month" as Mode, label: "月" },
              { value: "year" as Mode, label: "年" },
            ]}
          />
          <CurrencyPicker value={currency} onChange={setPicked} only={used.length > 0 ? used : undefined} />
        </div>
      </div>
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pb-4">
        <div className={`grid gap-3 ${mode === "month" ? "grid-cols-3" : "grid-cols-4"}`}>
          <Kpi label={mode === "month" ? "支出" : "全年支出"} value={fmt(spent)} />
          <Kpi label={mode === "month" ? "收入" : "全年收入"} value={fmt(income)} />
          <Kpi label="结余" value={money(balance, currency, true)} tone={balance > 0 ? "good" : balance < 0 ? "bad" : undefined} />
          {mode === "year" && <Kpi label="月均支出" value={monthsElapsed > 0 ? fmt(Math.round(spent / monthsElapsed)) : "—"} />}
        </div>
        {mode === "year" && (
          <Card title="每月趋势">
            <div className="pt-6">
              <ColumnChart
                labels={months.map((x) => `${Number(x.slice(5))}月`)}
                series={[
                  { name: "支出", color: "var(--chart-2)" },
                  { name: "收入", color: "var(--chart-1)" },
                ]}
                values={[months.map((x) => sum("out", totals.filter((t) => t.currency === currency && t.month === x))), months.map((x) => sum("in", totals.filter((t) => t.currency === currency && t.month === x)))]}
                format={fmt}
                axisFormat={(n) => moneyShort(n, currency)}
                title={(i) => monthTitle(months[i])}
              />
            </div>
          </Card>
        )}
        {share}
        {mode === "month" && (
          <Card
            title="预算"
            actions={
              <Button variant="ghost" onClick={() => setBudgetDialog(true)}>
                <Target size={14} /> 设预算
              </Button>
            }
          >
            {myBudgets.length === 0 ? (
              <p className="py-4 text-center text-sm text-muted-foreground">还没有设预算</p>
            ) : (
              <ul className="space-y-3">
                {myBudgets.map((b) => {
                  const used = byKind.get(b.kindId) ?? 0;
                  const over = used - b.amount;
                  return (
                    <li key={b.kindId} className="space-y-1">
                      <div className="flex items-baseline justify-between text-sm">
                        <span>{kindName.get(b.kindId)}</span>
                        <span className="tabular-nums">
                          {fmt(used)} <span className="text-muted-foreground">/ {fmt(b.amount)}</span>
                          {over > 0 ? <span className="ml-2 text-destructive">超出 {fmt(over)}</span> : <span className="ml-2 text-muted-foreground">还剩 {fmt(-over)}</span>}
                        </span>
                      </div>
                      <Meter value={used} max={b.amount} />
                    </li>
                  );
                })}
              </ul>
            )}
          </Card>
        )}
      </div>
      {budgetDialog && <BudgetDialog currency={currency} onClose={() => setBudgetDialog(false)} />}
    </>
  );
}
