// 报表 · 资产: how each account's balance and each holding's market value moved, for the accounts of
// the report's currency (no rates). x = only the days something was updated. Chips pick the series
// (hidden ones are remembered); colors follow the series' place in its list, not the selection.
import type { ReactNode } from "react";
import { MultiLineChart } from "@/shared/charts";
import { isStringArray, useSetting } from "@/shared/settings";
import { useAccounts, useAssets, useHoldings } from "./api";
import { CURRENCY_NAME, dayTick, dayTitle, money, moneyShort, type Currency } from "./money";

const COLORS = ["var(--chart-1)", "var(--chart-2)", "var(--chart-3)", "var(--chart-4)", "var(--chart-5)", "var(--chart-6)"];

type Series = { key: string; name: string; color: string; points: Map<string, number> };

function Trend(props: { title: string; series: Series[]; hidden: string[]; onToggle: (key: string) => void; currency: Currency; empty: string }) {
  const shown = props.series.filter((s) => !props.hidden.includes(s.key));
  const days = [...new Set(shown.flatMap((s) => [...s.points.keys()]))].sort();
  let body: ReactNode;
  if (props.series.length === 0) body = <p className="py-6 text-center text-sm text-muted-foreground">{props.empty}</p>;
  else if (days.length === 0) body = <p className="py-6 text-center text-sm text-muted-foreground">没有选中的</p>;
  else
    body = (
      <div className="pt-6">
        <MultiLineChart
          labels={days.map((d, i) => dayTick(d, days[i - 1]))}
          series={shown.map((s) => ({ key: s.key, name: s.name, color: s.color, values: days.map((d) => s.points.get(d) ?? null) }))}
          format={(n) => money(n, props.currency)}
          axisFormat={(n) => moneyShort(n, props.currency)}
          title={(i) => dayTitle(days[i])}
          labelEvery={Math.max(1, Math.ceil(days.length / 10))}
        />
      </div>
    );
  return (
    <section className="rounded-card border bg-card px-4 py-3">
      <h3 className="mb-2 text-sm text-muted-foreground">{props.title}</h3>
      {props.series.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {props.series.map((s) => {
            const on = !props.hidden.includes(s.key);
            return (
              <button
                key={s.key}
                type="button"
                aria-pressed={on}
                onClick={() => props.onToggle(s.key)}
                className={`flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs transition-colors duration-150 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring ${
                  on ? "" : "text-muted-foreground"
                }`}
              >
                <span className="size-2 rounded-full" style={{ background: on ? s.color : "var(--chart-other)" }} />
                {s.name}
              </button>
            );
          })}
        </div>
      )}
      {body}
    </section>
  );
}

export function AssetTrends(props: { currency: Currency }) {
  const { currency } = props;
  const { data: accounts = [] } = useAccounts();
  const { data: assets } = useAssets();
  const { data: holdings = [] } = useHoldings();
  const [hidden, setHidden] = useSetting<string[]>("ui.fin_trends_hidden", [], isStringArray);
  const toggle = (key: string) => setHidden(hidden.includes(key) ? hidden.filter((k) => k !== key) : [...hidden, key]);

  const balances = assets?.balances ?? [];
  const accountSeries: Series[] = accounts
    .filter((a) => a.currency === currency && balances.some((b) => b.accountId === a.id))
    .map((a, i) => ({
      key: `a${a.id}`,
      name: a.name,
      color: COLORS[i % COLORS.length],
      points: new Map(balances.filter((b) => b.accountId === a.id).map((b) => [b.day, b.amount])),
    }));

  const brokers = accounts.filter((a) => a.kind === "broker" && a.currency === currency && holdings.some((h) => h.accountId === a.id));
  const holdingSeries: Series[] = [];
  for (const a of brokers) {
    for (const h of holdings.filter((h) => h.accountId === a.id)) {
      const key = `h${a.id}:${h.name}`;
      let s = holdingSeries.find((x) => x.key === key);
      if (!s) {
        s = { key, name: brokers.length > 1 ? `${a.name} · ${h.name}` : h.name, color: COLORS[holdingSeries.length % COLORS.length], points: new Map() };
        holdingSeries.push(s);
      }
      s.points.set(h.day, h.value);
    }
  }

  return (
    <>
      <Trend title="账户变化" series={accountSeries} hidden={hidden} onToggle={toggle} currency={currency} empty={`还没有${CURRENCY_NAME[currency]}账户的余额`} />
      <Trend title="持仓变化" series={holdingSeries} hidden={hidden} onToggle={toggle} currency={currency} empty={`还没有${CURRENCY_NAME[currency]}券商账户的持仓`} />
    </>
  );
}
