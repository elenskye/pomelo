// Side panels: one 收支 entry, one 固定支出. With an account chosen, the currency is the account's.
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { localToday } from "@/shared/dates";
import { SidePanel } from "@/shared/panel";
import { DateInput } from "@/shared/DateInput";
import { Field, fieldClass, Segmented, TextInput } from "@/shared/ui";
import { useAccounts, useFinMutations, useFixed, type Entry, type Fixed, type Flow } from "./api";
import { AccountPicker, AmountInput, CurrencyPicker, FLOW_OPTIONS, KindPicker } from "./fields";
import { CURRENCY_NAME, money, moneyInput, nextOnOrAfter, parseMoney, type Currency } from "./money";

type Money = { flow: Flow; amount: string; currency: Currency; kindId: number | null; accountId: number | null; note: string };

/** Flow · 金额 + 币种 · 分类 · 账户 — the part entries and 固定支出 share. */
function MoneyFields<D extends Money>(props: { d: D; set: <K extends keyof D>(k: K, v: D[K]) => void; onManageKinds: () => void; autoFocus?: boolean }) {
  const { d, set } = props;
  const { data: accounts = [] } = useAccounts();
  const account = accounts.find((a) => a.id === d.accountId);
  return (
    <>
      <Field label="类型">
        <Segmented
          ariaLabel="支出还是收入"
          value={d.flow}
          onChange={(v) => {
            set("flow", v as D["flow"]);
            set("kindId", null as D["kindId"]);
          }}
          options={FLOW_OPTIONS}
        />
      </Field>
      <Field label="金额">
        <div className="flex items-center gap-2">
          <AmountInput autoFocus={props.autoFocus} value={d.amount} onChange={(v) => set("amount", v as D["amount"])} className="w-36" />
          {account ? (
            <span className="text-sm text-muted-foreground">{CURRENCY_NAME[account.currency]}</span>
          ) : (
            <CurrencyPicker value={d.currency} onChange={(c) => set("currency", c as D["currency"])} />
          )}
        </div>
      </Field>
      <Field label="分类">
        <KindPicker flow={d.flow} value={d.kindId} onChange={(v) => set("kindId", v as D["kindId"])} onManage={props.onManageKinds} />
      </Field>
      <Field label="账户">
        <AccountPicker
          value={d.accountId}
          onChange={(a) => {
            set("accountId", (a?.id ?? null) as D["accountId"]);
            if (a) set("currency", a.currency as D["currency"]);
          }}
        />
      </Field>
    </>
  );
}

function useDraft<D>(initial: D, onDirtyChange: (dirty: boolean) => void) {
  const [d, setD] = useState(initial);
  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = <K extends keyof D>(k: K, v: D[K]) => setD((x) => ({ ...x, [k]: v }));
  return [d, set, dirty] as const;
}

export function EntryPanel(props: {
  entry: Entry | null;
  defaultDay: string;
  defaultCurrency: Currency;
  onClose: () => void;
  onDone: () => void;
  onDirtyChange: (dirty: boolean) => void;
  onManageKinds: (flow: Flow) => void;
}) {
  const e = props.entry;
  const initial = useMemo(
    () => ({
      flow: e?.flow ?? ("out" as Flow),
      amount: moneyInput(e?.amount),
      currency: e?.currency ?? props.defaultCurrency,
      day: e?.day ?? props.defaultDay,
      kindId: e?.kindId ?? null,
      accountId: e?.accountId ?? null,
      note: e?.note ?? "",
    }),
    [e, props.defaultDay, props.defaultCurrency],
  );
  const [d, set, dirty] = useDraft(initial, props.onDirtyChange);
  const [error, setError] = useState<string | null>(null);
  const m = useFinMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const { data: fixed = [] } = useFixed();
  const from = e?.fixedId ? fixed.find((f) => f.id === e.fixedId) : undefined;

  const save = async (ev: FormEvent) => {
    ev.preventDefault();
    setError(null);
    const amount = parseMoney(d.amount);
    if (amount === null || amount <= 0) return setError("请填一个大于 0 的金额");
    const patch = { flow: d.flow, amount, currency: d.currency, day: d.day, kindId: d.kindId, accountId: d.accountId, note: d.note.trim() || null };
    try {
      if (e) {
        if (dirty) await m.updateEntry.mutateAsync({ id: e.id, patch });
        toast("已保存");
      } else {
        await m.createEntry.mutateAsync(patch);
        toast(`已记上 ${money(amount, d.currency)}`);
      }
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const del = async () => {
    if (!e || !(await confirm({ title: `删除这笔 ${money(e.amount, e.currency)}？`, confirmLabel: "删除", danger: true }))) return;
    m.deleteEntry.mutate(e.id, {
      onSuccess: () => {
        toast("已删除");
        props.onDone();
      },
    });
  };

  return (
    <SidePanel
      title={e ? "编辑记录" : "记一笔"}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={e ? () => void del() : undefined}
      error={error}
      meta={from ? `由固定支出「${from.name}」自动记上` : undefined}
      extra={confirmDialog}
    >
      <MoneyFields d={d} set={set} onManageKinds={() => props.onManageKinds(d.flow)} autoFocus />
      <Field label="日期">
        <div>
          <DateInput ariaLabel="日期" value={d.day} onChange={(v) => set("day", v)} />
        </div>
      </Field>
      <Field label="备注">
        <TextInput value={d.note} onChange={(ev) => set("note", ev.target.value)} maxLength={1000} />
      </Field>
    </SidePanel>
  );
}

type RuleKind = "monthly" | "yearly";

export function FixedPanel(props: {
  item: Fixed | null;
  defaultCurrency: Currency;
  onClose: () => void;
  onDone: () => void;
  onDirtyChange: (dirty: boolean) => void;
  onManageKinds: (flow: Flow) => void;
}) {
  const f = props.item;
  const today = localToday();
  const initial = useMemo(() => {
    const rule = f?.rule ?? `monthly:${Number(today.slice(8))}`;
    return {
      name: f?.name ?? "",
      flow: f?.flow ?? ("out" as Flow),
      amount: moneyInput(f?.amount),
      currency: f?.currency ?? props.defaultCurrency,
      kindId: f?.kindId ?? null,
      accountId: f?.accountId ?? null,
      rule,
      nextDay: f?.nextDay ?? nextOnOrAfter(rule, today),
      stopped: f?.stopped ?? false,
      note: f?.note ?? "",
    };
  }, [f, props.defaultCurrency, today]);
  const [d, set, dirty] = useDraft(initial, props.onDirtyChange);
  const [error, setError] = useState<string | null>(null);
  const m = useFinMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const [kind, arg = ""] = d.rule.split(":") as [RuleKind, string];
  const setRule = (rule: string) => {
    set("rule", rule);
    set("nextDay", nextOnOrAfter(rule, today));
  };

  const save = async (ev: FormEvent) => {
    ev.preventDefault();
    setError(null);
    const amount = parseMoney(d.amount);
    if (amount === null || amount <= 0) return setError("请填一个大于 0 的金额");
    const patch = {
      name: d.name,
      flow: d.flow,
      amount,
      currency: d.currency,
      kindId: d.kindId,
      accountId: d.accountId,
      rule: d.rule,
      nextDay: d.nextDay,
      stopped: d.stopped,
      note: d.note.trim() || null,
    };
    try {
      if (f) {
        if (dirty) await m.updateFixed.mutateAsync({ id: f.id, patch });
        toast("已保存");
      } else {
        await m.createFixed.mutateAsync(patch);
        toast(`已添加「${d.name.trim()}」`);
      }
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const del = async () => {
    if (!f || !(await confirm({ title: `删除固定支出「${f.name}」？`, message: "已经记上的那些笔会留着。", confirmLabel: "删除", danger: true }))) return;
    m.deleteFixed.mutate(f.id, {
      onSuccess: () => {
        toast("已删除");
        props.onDone();
      },
    });
  };
  const num = (v: string, max: number) => String(Math.max(1, Math.min(max, Number(v) || 1)));

  return (
    <SidePanel
      title={f ? "编辑固定支出" : "新固定支出"}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={f ? () => void del() : undefined}
      error={error}
      extra={confirmDialog}
    >
      <Field label="名称">
        <TextInput autoFocus value={d.name} onChange={(ev) => set("name", ev.target.value)} placeholder="必填" maxLength={50} />
      </Field>
      <MoneyFields d={d} set={set} onManageKinds={() => props.onManageKinds(d.flow)} />
      <Field label="周期">
        <div className="space-y-2">
          <Segmented
            ariaLabel="周期"
            value={kind}
            onChange={(k) => setRule(k === "monthly" ? `monthly:${Number(today.slice(8))}` : `yearly:${today.slice(5, 7)}-${today.slice(8)}`)}
            options={[
              { value: "monthly" as RuleKind, label: "每月" },
              { value: "yearly" as RuleKind, label: "每年" },
            ]}
          />
          <div className="flex items-center gap-2 text-sm">
            {kind === "monthly" ? (
              <>
                每月
                <input type="number" min={1} max={31} aria-label="几号" value={arg} onChange={(ev) => setRule(`monthly:${num(ev.target.value, 31)}`)} className={`${fieldClass} w-20`} />号
              </>
            ) : (
              <>
                每年
                <input
                  type="number"
                  min={1}
                  max={12}
                  aria-label="几月"
                  value={Number(arg.slice(0, 2))}
                  onChange={(ev) => setRule(`yearly:${num(ev.target.value, 12).padStart(2, "0")}-${arg.slice(3)}`)}
                  className={`${fieldClass} w-20`}
                />
                月
                <input
                  type="number"
                  min={1}
                  max={31}
                  aria-label="几日"
                  value={Number(arg.slice(3))}
                  onChange={(ev) => setRule(`yearly:${arg.slice(0, 2)}-${num(ev.target.value, 31).padStart(2, "0")}`)}
                  className={`${fieldClass} w-20`}
                />
                日
              </>
            )}
          </div>
        </div>
      </Field>
      <Field label="下次哪天记">
        <div>
          <DateInput ariaLabel="下次哪天记" value={d.nextDay} onChange={(v) => set("nextDay", v)} />
        </div>
      </Field>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={d.stopped} onChange={(ev) => set("stopped", ev.target.checked)} className="size-4 accent-[var(--primary)]" />
        停用
      </label>
      <Field label="备注">
        <TextInput value={d.note} onChange={(ev) => set("note", ev.target.value)} maxLength={1000} />
      </Field>
    </SidePanel>
  );
}
