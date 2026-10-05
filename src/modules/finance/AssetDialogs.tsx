// 资产 editors: 更新余额 (all accounts + rates for a month), an account's side panel, a broker's holdings.
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Plus, X } from "lucide-react";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { SidePanel } from "@/shared/panel";
import { Field, IconButton, fieldClass, inputClass, Segmented, TextArea, TextInput } from "@/shared/ui";
import { ACCOUNT_KINDS, useAccounts, useAssets, useFinMutations, useHoldings, type Account, type AccountKind } from "./api";
import { AmountInput, CurrencyPicker } from "./fields";
import { CURRENCY_NAME, money, moneyInput, monthTitle, parseMoney, thisMonth, type Currency } from "./money";

/** Latest value at or before `month` (this month's first, else the closest earlier one). */
function latest<T extends { month: string }>(rows: T[], month: string): T | undefined {
  return rows.filter((r) => r.month <= month).sort((a, b) => b.month.localeCompare(a.month))[0];
}

export function MonthDialog(props: { onClose: () => void }) {
  const { data: accounts = [] } = useAccounts();
  const { data: assets } = useAssets();
  const [month, setMonth] = useState(thisMonth());
  const [amounts, setAmounts] = useState<Record<number, string>>({});
  const [rates, setRates] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const m = useFinMutations();
  const toast = useToast();
  const active = accounts.filter((a) => !a.closed);
  const currencies = [...new Set(active.map((a) => a.currency))].filter((c) => c !== "CNY");

  // prefill from this month, else the latest earlier month
  useEffect(() => {
    if (!assets) return;
    setAmounts(Object.fromEntries(active.map((a) => [a.id, moneyInput(latest(assets.balances.filter((b) => b.accountId === a.id), month)?.amount)])));
    setRates(Object.fromEntries(currencies.map((c) => [c, String(latest(assets.rates.filter((r) => r.currency === c), month)?.rate ?? "")])));
  }, [assets, month, accounts]); // eslint-disable-line react-hooks/exhaustive-deps

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const balances = active.map((a) => ({ accountId: a.id, text: (amounts[a.id] ?? "").trim(), name: a.name }));
    const bad = balances.find((b) => b.text && parseMoney(b.text) === null);
    if (bad) return setError(`「${bad.name}」的余额不对`);
    const rateRows = currencies.map((c) => ({ currency: c, text: (rates[c] ?? "").trim() }));
    const badRate = rateRows.find((r) => r.text && !(Number(r.text) > 0));
    if (badRate) return setError(`${CURRENCY_NAME[badRate.currency]}的汇率不对`);
    try {
      await m.saveMonth.mutateAsync({
        month,
        balances: balances.map((b) => ({ accountId: b.accountId, amount: b.text ? parseMoney(b.text) : null })),
        rates: rateRows.map((r) => ({ currency: r.currency, rate: r.text ? Number(r.text) : null })),
      });
      toast(`已更新${monthTitle(month)}的余额`);
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Modal title="更新余额" onClose={props.onClose} onSubmit={save} error={error} submitLabel="保存" wide>
      <Field label="月份">
        <input type="month" aria-label="月份" value={month} onChange={(e) => e.target.value && setMonth(e.target.value)} className={`${fieldClass} w-auto`} />
      </Field>
      {active.length === 0 ? (
        <p className="text-sm text-muted-foreground">还没有账户</p>
      ) : (
        <div className="max-h-[42vh] space-y-2 overflow-y-auto pr-1">
          {active.map((a) => (
            <label key={a.id} className="grid grid-cols-[1fr_auto_10rem] items-center gap-3 text-sm">
              <span className="truncate">{a.name}</span>
              <span className="text-xs text-muted-foreground">{CURRENCY_NAME[a.currency]}</span>
              <AmountInput label={`${a.name}的余额`} value={amounts[a.id] ?? ""} onChange={(v) => setAmounts((x) => ({ ...x, [a.id]: v }))} />
            </label>
          ))}
        </div>
      )}
      {currencies.length > 0 && (
        <Field label="汇率（1 单位合多少人民币）">
          <div className="flex flex-wrap gap-x-5 gap-y-2">
            {currencies.map((c) => (
              <label key={c} className="flex items-center gap-2 text-sm whitespace-nowrap">
                1 {CURRENCY_NAME[c]} =
                <input
                  inputMode="decimal"
                  aria-label={`${CURRENCY_NAME[c]}汇率`}
                  value={rates[c] ?? ""}
                  onChange={(e) => setRates((x) => ({ ...x, [c]: e.target.value }))}
                  className={`${fieldClass} w-24 tabular-nums`}
                />
                人民币
              </label>
            ))}
          </div>
        </Field>
      )}
    </Modal>
  );
}

export function AccountPanel(props: { account: Account | null; onClose: () => void; onDone: () => void; onDirtyChange: (d: boolean) => void }) {
  const a = props.account;
  const initial = useMemo(
    () => ({ name: a?.name ?? "", kind: a?.kind ?? ("bank" as AccountKind), currency: a?.currency ?? ("CNY" as Currency), closed: a?.closed ?? false, note: a?.note ?? "" }),
    [a],
  );
  const [d, setD] = useState(initial);
  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = <K extends keyof typeof d>(k: K, v: (typeof d)[K]) => setD((x) => ({ ...x, [k]: v }));
  const [error, setError] = useState<string | null>(null);
  const m = useFinMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const { data: assets } = useAssets();
  const history = (assets?.balances ?? []).filter((b) => b.accountId === a?.id).reverse().slice(0, 12);

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const patch = { ...d, note: d.note.trim() || null };
    try {
      if (a) {
        if (dirty) await m.updateAccount.mutateAsync({ id: a.id, patch });
        toast("已保存");
      } else {
        await m.createAccount.mutateAsync(patch);
        toast(`已添加「${d.name.trim()}」`);
      }
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const del = async () => {
    if (!a) return;
    const ok = await confirm({ title: `删除账户「${a.name}」？`, message: "它的余额和持仓记录会一起删除；记在它名下的收支会留着。", confirmLabel: "删除", danger: true });
    if (ok)
      m.deleteAccount.mutate(a.id, {
        onSuccess: () => {
          toast("已删除");
          props.onDone();
        },
      });
  };

  return (
    <SidePanel title={a ? "编辑账户" : "新账户"} onClose={props.onClose} onSubmit={save} onDelete={a ? () => void del() : undefined} error={error} extra={confirmDialog}>
      <Field label="名称">
        <TextInput autoFocus value={d.name} onChange={(e) => set("name", e.target.value)} placeholder="必填" maxLength={30} />
      </Field>
      <Field label="类型">
        <Segmented ariaLabel="类型" value={d.kind} onChange={(v) => set("kind", v)} options={(Object.keys(ACCOUNT_KINDS) as AccountKind[]).map((k) => ({ value: k, label: ACCOUNT_KINDS[k] }))} />
      </Field>
      <Field label="币种">
        <CurrencyPicker value={d.currency} onChange={(c) => set("currency", c)} />
      </Field>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={d.closed} onChange={(e) => set("closed", e.target.checked)} className="size-4 accent-[var(--primary)]" />
        停用
      </label>
      <Field label="备注">
        <TextArea rows={2} value={d.note} onChange={(e) => set("note", e.target.value)} maxLength={1000} />
      </Field>
      {history.length > 0 && a && (
        <Field label="余额记录">
          <ul className="rounded-card border text-sm">
            {history.map((b) => (
              <li key={b.month} className="flex justify-between border-b px-3 py-1.5 last:border-b-0">
                <span className="text-muted-foreground">{monthTitle(b.month)}</span>
                <span className="tabular-nums">{money(b.amount, a.currency)}</span>
              </li>
            ))}
          </ul>
        </Field>
      )}
    </SidePanel>
  );
}

type Row = { name: string; quantity: string; value: string };

export function HoldingsDialog(props: { account: Account; onClose: () => void }) {
  const a = props.account;
  const { data: holdings = [] } = useHoldings();
  const [month, setMonth] = useState(thisMonth());
  const [rows, setRows] = useState<Row[]>([]);
  const [error, setError] = useState<string | null>(null);
  const m = useFinMutations();
  const toast = useToast();
  const mine = holdings.filter((h) => h.accountId === a.id);

  useEffect(() => {
    const src = latest(mine, month)?.month;
    setRows(
      mine
        .filter((h) => h.month === src)
        .map((h) => ({ name: h.name, quantity: h.quantity === null ? "" : String(h.quantity), value: moneyInput(h.value) })),
    );
  }, [month, holdings.length]); // eslint-disable-line react-hooks/exhaustive-deps

  const set = (i: number, k: keyof Row, v: string) => setRows((rs) => rs.map((r, j) => (j === i ? { ...r, [k]: v } : r)));
  const total = rows.reduce((s, r) => s + (parseMoney(r.value) ?? 0), 0);

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const filled = rows.filter((r) => r.name.trim() || r.value.trim());
    for (const r of filled) {
      if (!r.name.trim()) return setError("每一行都要有名称");
      if (parseMoney(r.value) === null) return setError(`「${r.name}」的市值不对`);
      if (r.quantity.trim() && !(Number(r.quantity) >= 0)) return setError(`「${r.name}」的数量不对`);
    }
    try {
      await m.saveHoldings.mutateAsync({
        accountId: a.id,
        month,
        rows: filled.map((r) => ({ name: r.name, quantity: r.quantity.trim() ? Number(r.quantity) : null, value: parseMoney(r.value)! })),
      });
      toast(`已更新「${a.name}」${monthTitle(month)}的持仓`);
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Modal title={`${a.name} · 持仓`} onClose={props.onClose} onSubmit={save} error={error} submitLabel="保存" wide>
      <Field label="月份">
        <input type="month" aria-label="月份" value={month} onChange={(e) => e.target.value && setMonth(e.target.value)} className={`${fieldClass} w-auto`} />
      </Field>
      <div className="space-y-2">
        <div className="grid grid-cols-[1fr_6rem_9rem_2rem] gap-2 text-xs text-muted-foreground">
          <span>名称</span>
          <span>数量</span>
          <span>市值（{CURRENCY_NAME[a.currency]}）</span>
        </div>
        {rows.map((r, i) => (
          <div key={i} className="grid grid-cols-[1fr_6rem_9rem_2rem] items-center gap-2">
            <TextInput aria-label="名称" value={r.name} onChange={(e) => set(i, "name", e.target.value)} maxLength={50} />
            <input inputMode="decimal" aria-label="数量" value={r.quantity} onChange={(e) => set(i, "quantity", e.target.value)} className={`${inputClass} tabular-nums`} />
            <AmountInput label="市值" value={r.value} onChange={(v) => set(i, "value", v)} />
            <IconButton label="删掉这一行" danger onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))}>
              <X size={14} />
            </IconButton>
          </div>
        ))}
        <button type="button" onClick={() => setRows((rs) => [...rs, { name: "", quantity: "", value: "" }])} className="flex items-center gap-1 text-sm text-primary hover:underline">
          <Plus size={14} /> 加一行
        </button>
      </div>
      <p className="text-right text-sm text-muted-foreground">
        合计 <b className="text-base text-foreground tabular-nums">{money(total, a.currency)}</b>
      </p>
    </Modal>
  );
}
