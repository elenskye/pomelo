// 记一周小额: one sum per 支出分类 in one go (one entry each, one undo step).
import { useState, type FormEvent } from "react";
import { localToday } from "@/shared/dates";
import { errorText, useToast } from "@/shared/feedback";
import { useKinds } from "@/shared/kinds/api";
import { Modal } from "@/shared/Modal";
import { Field, fieldClass, TextInput } from "@/shared/ui";
import { OUT_KINDS, useAccounts, useFinMutations } from "./api";
import { AccountPicker, AmountInput, CurrencyPicker } from "./fields";
import { CURRENCY_NAME, money, parseMoney, type Currency } from "./money";

export function WeekDialog(props: { defaultCurrency: Currency; onClose: () => void }) {
  const { data: kinds = [] } = useKinds(OUT_KINDS);
  const { data: accounts = [] } = useAccounts();
  const [day, setDay] = useState(localToday());
  const [currency, setCurrency] = useState<Currency>(props.defaultCurrency);
  const [accountId, setAccountId] = useState<number | null>(null);
  const [note, setNote] = useState("本周小额");
  const [amounts, setAmounts] = useState<Record<number, string>>({});
  const [error, setError] = useState<string | null>(null);
  const m = useFinMutations();
  const toast = useToast();
  const account = accounts.find((a) => a.id === accountId);
  const parsed = kinds.map((k) => ({ k, cents: (amounts[k.id] ?? "").trim() ? parseMoney(amounts[k.id]) : 0 }));
  const total = parsed.reduce((s, x) => s + (x.cents && x.cents > 0 ? x.cents : 0), 0);

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const bad = parsed.find((x) => x.cents === null || (x.cents ?? 0) < 0);
    if (bad) return setError(`「${bad.k.name}」的金额不对`);
    const rows = parsed.filter((x) => (x.cents ?? 0) > 0);
    if (rows.length === 0) return setError("至少填一个分类的金额");
    try {
      await m.createEntries.mutateAsync(rows.map((x) => ({ flow: "out", amount: x.cents!, currency, day, kindId: x.k.id, accountId, note: note.trim() || null })));
      toast(`已记上 ${rows.length} 笔，共 ${money(total, currency)}`);
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Modal title="记一周小额" onClose={props.onClose} onSubmit={save} error={error} submitLabel="记上" wide>
      <div className="grid grid-cols-2 gap-4">
        <Field label="日期">
          <input type="date" aria-label="日期" value={day} onChange={(e) => e.target.value && setDay(e.target.value)} className={`${fieldClass} w-auto`} />
        </Field>
        <Field label="账户">
          <AccountPicker
            value={accountId}
            onChange={(a) => {
              setAccountId(a?.id ?? null);
              if (a) setCurrency(a.currency);
            }}
          />
        </Field>
      </div>
      <Field label="币种">{account ? <p className="text-sm">{CURRENCY_NAME[account.currency]}</p> : <CurrencyPicker value={currency} onChange={setCurrency} />}</Field>
      <div className="grid max-h-[40vh] grid-cols-2 gap-x-4 gap-y-2 overflow-y-auto pr-1">
        {kinds.map((k) => (
          <label key={k.id} className="flex items-center gap-2 text-sm">
            <span className="w-16 shrink-0 truncate">{k.name}</span>
            <AmountInput label={k.name} value={amounts[k.id] ?? ""} onChange={(v) => setAmounts((a) => ({ ...a, [k.id]: v }))} />
          </label>
        ))}
      </div>
      <div className="flex items-center justify-between gap-4">
        <Field label="备注">
          <TextInput value={note} onChange={(e) => setNote(e.target.value)} maxLength={1000} className="w-56" />
        </Field>
        <p className="text-sm text-muted-foreground">
          合计 <b className="text-base text-foreground tabular-nums">{money(total, currency)}</b>
        </p>
      </div>
    </Modal>
  );
}
