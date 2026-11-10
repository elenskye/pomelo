// Side panels: one 收支 entry (a period from 记小额 edits its two days), one 固定支出.
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { localToday } from "@/shared/dates";
import { SidePanel } from "@/shared/panel";
import { DateInput } from "@/shared/DateInput";
import { Field, Segmented, TextInput } from "@/shared/ui";
import { useFinMutations, useFixed, type Entry, type Fixed, type Flow } from "./api";
import { Dropdown } from "@/shared/Dropdown";
import { useSources, useRefundCandidates } from "./importApi";
import { AmountInput, CurrencyPicker, FLOW_OPTIONS, KindPicker } from "./fields";
import { money, moneyInput, nextOnOrAfter, parseMoney, type Currency } from "./money";
import { Polish, QuickFill, useAssistOn, useFilled, type QuickFillResult } from "@/modules/youzi/Assist";

type Money = { flow: Flow; amount: string; currency: Currency; kindId: number | null; note: string };

/** Flow · 金额 + 币种 · 分类 — the part entries and 固定支出 share. */
type Filled = ReturnType<typeof useFilled<string>>;

function MoneyFields<D extends Money>(props: { d: D; set: <K extends keyof D>(k: K, v: D[K]) => void; onManageKinds: () => void; autoFocus?: boolean; filled?: Filled }) {
  const { d, set } = props;
  const halo = (k: string) => props.filled?.halo(k) ?? "";
  return (
    <>
      <Field label="类型">
        <div className={halo("flow")}>
          <Segmented
            ariaLabel="支出还是收入"
            value={d.flow}
            onChange={(v) => {
              set("flow", v as D["flow"]);
              set("kindId", null as D["kindId"]);
            }}
            options={FLOW_OPTIONS}
          />
        </div>
      </Field>
      <Field label="金额">
        <div className="flex flex-wrap items-center gap-2">
          <AmountInput autoFocus={props.autoFocus} value={d.amount} onChange={(v) => set("amount", v as D["amount"])} className={`w-36 ${props.filled?.cls("amount") ?? ""}`} />
          <div className={halo("currency")}>
            <CurrencyPicker value={d.currency} onChange={(c) => set("currency", c as D["currency"])} />
          </div>
        </div>
      </Field>
      <Field label="分类">
        <div className={halo("kindId")}>
          <KindPicker flow={d.flow} value={d.kindId} onChange={(v) => set("kindId", v as D["kindId"])} onManage={props.onManageKinds} />
        </div>
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
  return [d, set, dirty, setD] as const;
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
      isRefund: e?.isRefund ?? false,
      refundOf: e?.refundOf ?? null,
      flow: e?.flow ?? ("out" as Flow),
      amount: moneyInput(e?.amount),
      currency: e?.currency ?? props.defaultCurrency,
      day: e?.day ?? props.defaultDay,
      endDay: e?.endDay ?? "",
      kindId: e?.kindId ?? null,
      note: e?.note ?? "",
    }),
    [e, props.defaultDay, props.defaultCurrency],
  );
  const [d, setRaw, dirty, setD] = useDraft(initial, props.onDirtyChange);
  // 一句话填好 (new) / 改好 (editing): filled fields show until edited by hand
  const assistOn = useAssistOn();
  const quick = assistOn;
  const filled = useFilled<string>();
  const set = <K extends keyof typeof d>(k: K, v: (typeof d)[K]) => {
    filled.clear(k);
    setRaw(k, v);
    if (k === "flow" && v === "in") { setRaw("isRefund", false); setRaw("refundOf", null); }
  };
  const fill = (f: QuickFillResult) => {
    const next = { ...d };
    const keys: string[] = [];
    if (f.flow && f.flow !== d.flow) (next.flow = f.flow), (next.kindId = null), keys.push("flow");
    if (f.amount != null) (next.amount = moneyInput(f.amount)), keys.push("amount");
    if (f.currency) (next.currency = f.currency), keys.push("currency");
    if (f.day) (next.day = f.day), keys.push("day");
    if (f.kindId != null) (next.kindId = f.kindId), keys.push("kindId");
    if (f.note) (next.note = f.note), keys.push("note");
    if (f.refund && next.flow === "out" && !e?.fixedId && !next.endDay) (next.isRefund = true), keys.push("isRefund");
    if (f.clear.includes("note")) (next.note = ""), keys.push("note");
    if (f.clear.includes("kind")) (next.kindId = null), keys.push("kindId");
    setD(next);
    filled.mark(keys);
  };
  const [error, setError] = useState<string | null>(null);
  const m = useFinMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const { data: fixed = [] } = useFixed();
  const sources = useSources(e?.id);
  const candidates = useRefundCandidates(d.currency, d.day, d.isRefund);
  const from = e?.fixedId ? fixed.find((f) => f.id === e.fixedId) : undefined;

  const save = async (ev: FormEvent) => {
    ev.preventDefault();
    setError(null);
    const amount = parseMoney(d.amount);
    if (amount === null || amount <= 0) return setError("请填一个大于 0 的金额");
    const patch = { isRefund: d.isRefund, refundOf: d.isRefund ? d.refundOf : null, flow: d.flow, amount, currency: d.currency, day: d.day, endDay: d.endDay || null, kindId: d.kindId, note: d.note.trim() || null };
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
      {quick && <QuickFill module="finance" record={e?.id} onFill={fill} autoFocus={!e} />}
      <MoneyFields d={d} set={set} onManageKinds={() => props.onManageKinds(d.flow)} autoFocus={!quick || !!e} filled={filled} />
      {d.flow === "out" && !from && <Field label="退款"><label className={`flex items-center gap-2 text-sm ${filled.halo("isRefund")}`}><input type="checkbox" checked={d.isRefund} onChange={(ev) => { set("isRefund", ev.target.checked); set("refundOf", null); if (ev.target.checked) set("endDay", ""); }} />冲减支出</label></Field>}
      {d.isRefund && <Field label="原支出"><Dropdown ariaLabel="原支出" value={d.refundOf} options={[{ value: null as number | null, label: "未关联" }, ...(candidates.data ?? []).filter((x) => x.id !== e?.id).map((x) => ({ value: x.id as number | null, label: `${x.day} ${money(x.amount, x.currency)} ${x.note ?? ""}` }))]} onChange={(refundOf) => { set("refundOf", refundOf); if (refundOf) set("kindId", candidates.data?.find((x) => x.id === refundOf)?.kindId ?? null); }} />{candidates.error && <p className="text-sm text-destructive">{errorText(candidates.error)}</p>}</Field>}
      {initial.endDay && !d.isRefund ? (
        <Field label="时间段">
          <div className="flex items-center gap-2">
            <DateInput ariaLabel="开始日期" value={d.day} onChange={(v) => set("day", v)} max={d.endDay} />
            <span className="text-muted-foreground">到</span>
            <DateInput ariaLabel="结束日期" value={d.endDay} onChange={(v) => set("endDay", v)} min={d.day} />
          </div>
        </Field>
      ) : (
        <Field label="日期">
          <div className={filled.halo("day")}>
            <DateInput ariaLabel="日期" value={d.day} onChange={(v) => set("day", v)} />
          </div>
        </Field>
      )}
      {sources.error && <p className="text-sm text-destructive">{errorText(sources.error)}</p>}
      {sources.data && sources.data.length > 0 && <details><summary className="cursor-pointer text-sm">账单来源（{sources.data.length}）</summary>{sources.data.map((r) => <div key={r.id} className="mt-3 text-xs"><p className="font-bold">{r.fileName} · {r.source.sheet} · 第 {r.source.rowNumber} 行</p>{r.source.raw.map(([label, value], i) => <p key={i} className="mt-1 break-all"><span className="text-muted-foreground">{label}：</span>{value || "—"}</p>)}</div>)}</details>}
      <Field label="备注">
        <Polish module="finance" value={d.note} onChange={(v) => set("note", v)}>
          <TextInput value={d.note} onChange={(ev) => set("note", ev.target.value)} maxLength={1000} className={`pr-8 ${filled.cls("note")}`} />
        </Polish>
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
                <TextInput type="number" min={1} max={31} aria-label="几号" value={arg} onChange={(ev) => setRule(`monthly:${num(ev.target.value, 31)}`)} className="w-20" />号
              </>
            ) : (
              <>
                每年
                <TextInput
                  type="number"
                  min={1}
                  max={12}
                  aria-label="几月"
                  value={Number(arg.slice(0, 2))}
                  onChange={(ev) => setRule(`yearly:${num(ev.target.value, 12).padStart(2, "0")}-${arg.slice(3)}`)}
                  className="w-20"
                />
                月
                <TextInput
                  type="number"
                  min={1}
                  max={31}
                  aria-label="几日"
                  value={Number(arg.slice(3))}
                  onChange={(ev) => setRule(`yearly:${arg.slice(0, 2)}-${num(ev.target.value, 31).padStart(2, "0")}`)}
                  className="w-20"
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
