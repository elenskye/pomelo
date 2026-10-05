// 资产: total assets in CNY (each month's balances × that month's rates) and its curve, the
// accounts with their latest balance, and each broker account's latest holdings.
import { Plus, RefreshCw } from "lucide-react";
import { LineChart } from "@/shared/charts";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { useSortable } from "@/shared/sortable";
import { Button } from "@/shared/ui";
import { useState } from "react";
import { ACCOUNT_KINDS, useAccounts, useAssets, useFinMutations, useHoldings, type Account } from "./api";
import { AccountPanel, HoldingsDialog, MonthDialog } from "./AssetDialogs";
import { CURRENCY_NAME, money, moneyShort, monthTick, monthTitle } from "./money";

export function AssetsView() {
  const { data: accounts = [] } = useAccounts();
  const { data: assets } = useAssets();
  const { data: holdings = [] } = useHoldings();
  const m = useFinMutations();
  const panel = usePanelSelection<string>();
  const [monthDialog, setMonthDialog] = useState(false);
  const [holdingsFor, setHoldingsFor] = useState<Account | null>(null);
  const sort = useSortable<HTMLTableSectionElement>((ids) => m.reorderAccounts.mutate(ids));
  usePageKeys({ onNew: () => void panel.request("new"), onEscape: panel.selection !== null ? () => void panel.request(null) : undefined });

  const totals = assets?.totals ?? [];
  const complete = totals.filter((t) => t.totalCny !== null);
  const last = complete[complete.length - 1];
  const prev = complete[complete.length - 2];
  const newest = totals[totals.length - 1];
  const delta = last && prev ? last.totalCny! - prev.totalCny! : null;
  const latestRates = assets?.rates.filter((r) => r.month === assets.rates[assets.rates.length - 1]?.month) ?? [];
  const editing = panel.selection === "new" ? null : accounts.find((a) => `a${a.id}` === panel.selection);
  const brokers = accounts.filter((a) => a.kind === "broker" && !a.closed);

  return (
    <>
      <div className="flex shrink-0 items-start justify-between gap-4 pb-3">
        <div>
          <p className="text-sm text-muted-foreground">总资产{last && ` · ${monthTitle(last.month)} · 折合人民币`}</p>
          <p className="text-4xl font-bold">{last ? money(last.totalCny!, "CNY") : "—"}</p>
          {delta !== null && (
            <p className={`text-sm tabular-nums ${delta >= 0 ? "text-[var(--status-done-fg)]" : "text-destructive"}`}>
              比上个月 {delta >= 0 ? "+" : "−"}
              {money(Math.abs(delta), "CNY")}
            </p>
          )}
          {newest && newest.totalCny === null && (
            <p className="text-sm text-destructive">
              {monthTitle(newest.month)}还缺{newest.missing.map((c) => CURRENCY_NAME[c]).join("、")}的汇率
            </p>
          )}
        </div>
        <div className="flex shrink-0 gap-2">
          <Button onClick={() => void panel.request("new")}>
            <Plus size={14} /> 新账户
          </Button>
          <Button variant="primary" disabled={accounts.every((a) => a.closed)} onClick={() => setMonthDialog(true)}>
            <RefreshCw size={14} /> 更新余额
          </Button>
        </div>
      </div>
      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto pb-4">
        {totals.length > 1 && (
          <section className="rounded-card border bg-card px-4 pt-8 pb-2">
            <LineChart
              name="总资产（人民币）"
              baseline="auto"
              labels={totals.map((t, i) => monthTick(t.month, i === 0))}
              values={totals.map((t) => t.totalCny)}
              format={(n) => money(n, "CNY")}
              axisFormat={(n) => moneyShort(n, "CNY")}
              title={(i) => monthTitle(totals[i].month)}
              labelEvery={totals.length > 14 ? 3 : 1}
            />
          </section>
        )}
        <section>
          <h3 className="mb-1.5 px-1 text-sm text-muted-foreground">账户</h3>
          {accounts.length === 0 ? (
            <p className="rounded-card border bg-card px-4 py-6 text-center text-muted-foreground">还没有账户</p>
          ) : (
            <table className="w-full overflow-hidden rounded-card border bg-card text-sm">
              <thead className="border-b text-xs text-muted-foreground">
                <tr>
                  <th className="px-3 py-2 text-left font-normal">名称</th>
                  <th className="w-20 px-3 py-2 text-left font-normal">类型</th>
                  <th className="w-20 px-3 py-2 text-left font-normal">币种</th>
                  <th className="w-40 px-3 py-2 text-right font-normal">最近余额</th>
                  <th className="w-32 px-3 py-2 text-left font-normal">月份</th>
                </tr>
              </thead>
              <tbody ref={sort.listRef}>
                {accounts.map((a) => (
                  <tr
                    key={a.id}
                    {...sort.item(a.id)}
                    {...(accounts.length > 1 ? sort.handle(a.id) : {})}
                    onClick={() => void panel.request(`a${a.id}`)}
                    className={`cursor-pointer border-b bg-card transition-colors duration-150 last:border-b-0 ${a.closed ? "text-muted-foreground" : ""} ${
                      panel.selection === `a${a.id}` ? "bg-accent" : "hover:bg-muted/70"
                    }`}
                  >
                    <td className="px-3 py-2">
                      {a.name}
                      {a.closed && <span className="ml-2 rounded bg-muted px-1.5 text-xs">停用</span>}
                    </td>
                    <td className="px-3 py-2">{ACCOUNT_KINDS[a.kind]}</td>
                    <td className="px-3 py-2">{CURRENCY_NAME[a.currency]}</td>
                    <td className="px-3 py-2 text-right tabular-nums">{a.lastAmount === null ? "—" : money(a.lastAmount, a.currency)}</td>
                    <td className="px-3 py-2 text-muted-foreground">{a.lastMonth ? monthTitle(a.lastMonth) : ""}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {latestRates.length > 0 && (
            <p className="mt-2 px-1 text-xs text-muted-foreground tabular-nums">
              汇率（{monthTitle(latestRates[0].month)}）：{latestRates.map((r) => `1 ${CURRENCY_NAME[r.currency]} = ¥${r.rate}`).join(" · ")}
            </p>
          )}
        </section>
        {brokers.map((a) => {
          const mine = holdings.filter((h) => h.accountId === a.id);
          const month = mine[mine.length - 1]?.month;
          const rows = mine.filter((h) => h.month === month);
          return (
            <section key={a.id}>
              <div className="mb-1.5 flex items-center justify-between px-1">
                <h3 className="text-sm text-muted-foreground">
                  {a.name} · 持仓{month && `（${monthTitle(month)}）`}
                </h3>
                <Button variant="ghost" onClick={() => setHoldingsFor(a)}>
                  更新持仓
                </Button>
              </div>
              {rows.length === 0 ? (
                <p className="rounded-card border bg-card px-4 py-4 text-center text-sm text-muted-foreground">还没有记持仓</p>
              ) : (
                <table className="w-full overflow-hidden rounded-card border bg-card text-sm">
                  <tbody>
                    {rows.map((h) => (
                      <tr key={h.id} className="border-b last:border-b-0">
                        <td className="px-3 py-2">{h.name}</td>
                        <td className="w-32 px-3 py-2 text-right text-muted-foreground tabular-nums">{h.quantity ?? ""}</td>
                        <td className="w-40 px-3 py-2 text-right tabular-nums">{money(h.value, a.currency)}</td>
                      </tr>
                    ))}
                    <tr className="bg-muted/40">
                      <td className="px-3 py-2 text-muted-foreground">合计</td>
                      <td />
                      <td className="px-3 py-2 text-right font-bold tabular-nums">{money(rows.reduce((s, h) => s + h.value, 0), a.currency)}</td>
                    </tr>
                  </tbody>
                </table>
              )}
            </section>
          );
        })}
      </div>

      {(panel.selection === "new" || editing) && (
        <AccountPanel key={panel.selection} account={editing ?? null} onClose={() => void panel.request(null)} onDone={panel.done} onDirtyChange={panel.setDirty} />
      )}
      {monthDialog && <MonthDialog onClose={() => setMonthDialog(false)} />}
      {holdingsFor && <HoldingsDialog account={holdingsFor} onClose={() => setHoldingsFor(null)} />}
      {panel.confirmDialog}
    </>
  );
}
