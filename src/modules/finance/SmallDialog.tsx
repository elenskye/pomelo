// 记小额: a period (start … end) and one sum per 支出分类 → one entry each with that period, one undo
// step. Reports spread each sum evenly over the period's days (finance.rs `spread`).
import { useEffect, useState, type FormEvent } from "react";
import { addDays, localToday } from "@/shared/dates";
import { DateInput } from "@/shared/DateInput";
import { errorText, useToast } from "@/shared/feedback";
import { useKinds } from "@/shared/kinds/api";
import { Modal } from "@/shared/Modal";
import { Field, TextInput } from "@/shared/ui";
import { OUT_KINDS, useFinMutations, useLastPeriodEnd } from "./api";
import { AmountInput, CurrencyPicker } from "./fields";
import { money, parseMoney, type Currency } from "./money";

const dayCount = (a: string, b: string) => Math.round((Date.parse(`${b}T00:00:00Z`) - Date.parse(`${a}T00:00:00Z`)) / 864e5) + 1;

export function SmallDialog(props: { defaultCurrency: Currency; onClose: () => void }) {
  const { data: kinds = [] } = useKinds(OUT_KINDS);
  const lastEnd = useLastPeriodEnd();
  const today = localToday();
  const [start, setStart] = useState(addDays(today, -6));
  const [end, setEnd] = useState(today);
  const [touched, setTouched] = useState(false);
  const [currency, setCurrency] = useState<Currency>(props.defaultCurrency);
  const [note, setNote] = useState("");
  const [amounts, setAmounts] = useState<Record<number, string>>({});
  const [error, setError] = useState<string | null>(null);
  const m = useFinMutations();
  const toast = useToast();
  const parsed = kinds.map((k) => ({ k, cents: (amounts[k.id] ?? "").trim() ? parseMoney(amounts[k.id]) : 0 }));
  const total = parsed.reduce((s, x) => s + (x.cents && x.cents > 0 ? x.cents : 0), 0);

  // pick up where the last period ended
  useEffect(() => {
    const next = lastEnd.data ? addDays(lastEnd.data, 1) : null;
    if (!touched && next && next <= today) setStart(next);
  }, [lastEnd.data]); // eslint-disable-line react-hooks/exhaustive-deps

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    if (end < start) return setError("结束日期不能早于开始日期");
    const bad = parsed.find((x) => x.cents === null || (x.cents ?? 0) < 0);
    if (bad) return setError(`「${bad.k.name}」的金额不对`);
    const rows = parsed.filter((x) => (x.cents ?? 0) > 0);
    if (rows.length === 0) return setError("至少填一个分类的金额");
    try {
      await m.createEntries.mutateAsync(rows.map((x) => ({ flow: "out", amount: x.cents!, currency, day: start, endDay: end, kindId: x.k.id, note: note.trim() || null })));
      toast(`已记上 ${rows.length} 笔，共 ${money(total, currency)}`);
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Modal title="记小额" onClose={props.onClose} onSubmit={save} error={error} submitLabel="记上" wide>
      <Field label="时间段">
        <div className="flex items-center gap-2">
          <DateInput
            ariaLabel="开始日期"
            value={start}
            max={end}
            onChange={(v) => {
              setTouched(true);
              setStart(v);
            }}
          />
          <span className="text-muted-foreground">到</span>
          <DateInput ariaLabel="结束日期" value={end} min={start} onChange={setEnd} />
          {end >= start && <span className="text-sm text-muted-foreground">共 {dayCount(start, end)} 天</span>}
        </div>
      </Field>
      <Field label="币种">
        <CurrencyPicker value={currency} onChange={setCurrency} />
      </Field>
      <div className="-m-1 grid max-h-[40vh] grid-cols-2 gap-x-4 gap-y-2 overflow-y-auto p-1 pr-2">
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
