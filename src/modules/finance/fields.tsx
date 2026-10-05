// Form pieces shared by 钱来钱往's panels and dialogs.
import { Dropdown } from "@/shared/Dropdown";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { fieldClass, Segmented } from "@/shared/ui";
import { kindsModule, useAccounts, type Account, type Flow } from "./api";
import { CURRENCIES, CURRENCY_NAME, type Currency } from "./money";

export function AmountInput(props: { value: string; onChange: (v: string) => void; label?: string; autoFocus?: boolean; className?: string }) {
  return (
    <input
      inputMode="decimal"
      aria-label={props.label ?? "金额"}
      autoFocus={props.autoFocus}
      value={props.value}
      onChange={(e) => props.onChange(e.target.value)}
      className={`${fieldClass} tabular-nums ${props.className ?? "w-full"}`}
    />
  );
}

export function CurrencyPicker(props: { value: Currency; onChange: (c: Currency) => void; only?: Currency[] }) {
  return (
    <Segmented
      ariaLabel="币种"
      value={props.value}
      onChange={props.onChange}
      options={(props.only ?? CURRENCIES).map((c) => ({ value: c, label: CURRENCY_NAME[c] }))}
    />
  );
}

export const FLOW_OPTIONS: { value: Flow; label: string }[] = [
  { value: "out", label: "支出" },
  { value: "in", label: "收入" },
];

export function KindPicker(props: { flow: Flow; value: number | null; onChange: (v: number | null) => void; onManage: () => void }) {
  const { data: kinds = [] } = useKinds(kindsModule(props.flow));
  return (
    <div>
      <Dropdown
        ariaLabel="分类"
        value={props.value}
        onChange={props.onChange}
        options={[...kinds.map((k) => ({ value: k.id as number | null, label: k.name })), { value: null, label: UNCATEGORIZED }]}
        actions={[{ label: "管理分类…", onSelect: props.onManage }]}
        menuMaxHeight={320}
      />
    </div>
  );
}

/** 不填 or an account (closed ones only while already chosen). Picking one sets the currency. */
export function AccountPicker(props: { value: number | null; onChange: (a: Account | null) => void }) {
  const { data: accounts = [] } = useAccounts();
  const shown = accounts.filter((a) => !a.closed || a.id === props.value);
  return (
    <div>
      <Dropdown
        ariaLabel="账户"
        value={props.value}
        onChange={(id) => props.onChange(accounts.find((a) => a.id === id) ?? null)}
        options={[{ value: null as number | null, label: "不填" }, ...shown.map((a) => ({ value: a.id as number | null, label: `${a.name} · ${CURRENCY_NAME[a.currency]}` }))]}
        menuMaxHeight={320}
      />
    </div>
  );
}
