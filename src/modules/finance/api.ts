// Typed wrappers for Rust fin_* commands (src-tauri/src/modules/finance.rs) + React Query hooks.
// Rules (currency per account, posting 固定支出, total assets) live in Rust; the UI only renders.
import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { KINDS_KEY } from "@/shared/kinds/api";
import type { Currency } from "./money";

export type Flow = "out" | "in";
export const OUT_KINDS = "fin_out";
export const IN_KINDS = "fin_in";
export const kindsModule = (f: Flow) => (f === "out" ? OUT_KINDS : IN_KINDS);

export type AccountKind = "bank" | "wallet" | "broker";
export const ACCOUNT_KINDS: Record<AccountKind, string> = { bank: "银行", wallet: "钱包", broker: "券商" };

export type Account = {
  id: number;
  name: string;
  kind: AccountKind;
  currency: Currency;
  closed: boolean;
  note: string | null;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
  lastMonth: string | null;
  lastAmount: number | null;
};
export type AccountPatch = Partial<Pick<Account, "name" | "kind" | "currency" | "closed" | "note">>;
export type Balance = { accountId: number; month: string; amount: number };
export type Rate = { month: string; currency: Currency; rate: number };
export type MonthTotal = { month: string; totalCny: number | null; missing: Currency[] };
export type Assets = { balances: Balance[]; rates: Rate[]; totals: MonthTotal[] };
export type Holding = { id: number; accountId: number; month: string; name: string; quantity: number | null; value: number };
export type HoldingInput = { name: string; quantity: number | null; value: number };

export type Entry = {
  id: number;
  flow: Flow;
  amount: number;
  currency: Currency;
  day: string;
  kindId: number | null;
  accountId: number | null;
  fixedId: number | null;
  note: string | null;
  createdAt: string;
  updatedAt: string;
};
export type EntryPatch = Partial<Pick<Entry, "flow" | "amount" | "currency" | "day" | "kindId" | "accountId" | "note">>;

export type Fixed = {
  id: number;
  name: string;
  flow: Flow;
  amount: number;
  currency: Currency;
  kindId: number | null;
  accountId: number | null;
  /** monthly:<1-31> | yearly:<MM-DD> */
  rule: string;
  nextDay: string;
  stopped: boolean;
  note: string | null;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
};
export type FixedPatch = Partial<Pick<Fixed, "name" | "flow" | "amount" | "currency" | "kindId" | "accountId" | "rule" | "nextDay" | "stopped" | "note">>;

export type Budget = { kindId: number; currency: Currency; amount: number };
/** Entry sums by month × currency × flow × 分类. */
export type Total = { month: string; currency: Currency; flow: Flow; kindId: number | null; amount: number; count: number };

const api = {
  accounts: () => invoke<Account[]>("fin_accounts"),
  createAccount: (patch: AccountPatch) => invoke<Account>("fin_account_create", { patch }),
  updateAccount: (v: { id: number; patch: AccountPatch }) => invoke<Account>("fin_account_update", v),
  deleteAccount: (id: number) => invoke<void>("fin_account_delete", { id }),
  reorderAccounts: (ids: number[]) => invoke<void>("fin_accounts_reorder", { ids }),
  assets: () => invoke<Assets>("fin_assets"),
  saveMonth: (v: { month: string; balances: { accountId: number; amount: number | null }[]; rates: { currency: Currency; rate: number | null }[] }) =>
    invoke<void>("fin_month_save", v),
  holdings: () => invoke<Holding[]>("fin_holdings"),
  saveHoldings: (v: { accountId: number; month: string; rows: HoldingInput[] }) => invoke<void>("fin_holdings_save", v),
  entries: (month: string) => invoke<Entry[]>("fin_entries", { month }),
  createEntry: (patch: EntryPatch) => invoke<Entry>("fin_entry_create", { patch }),
  createEntries: (patches: EntryPatch[]) => invoke<number>("fin_entries_create", { patches }),
  updateEntry: (v: { id: number; patch: EntryPatch }) => invoke<Entry>("fin_entry_update", v),
  deleteEntry: (id: number) => invoke<void>("fin_entry_delete", { id }),
  fixed: () => invoke<Fixed[]>("fin_fixed"),
  createFixed: (patch: FixedPatch) => invoke<Fixed>("fin_fixed_create", { patch }),
  updateFixed: (v: { id: number; patch: FixedPatch }) => invoke<Fixed>("fin_fixed_update", v),
  deleteFixed: (id: number) => invoke<void>("fin_fixed_delete", { id }),
  reorderFixed: (ids: number[]) => invoke<void>("fin_fixed_reorder", { ids }),
  budgets: () => invoke<Budget[]>("fin_budgets"),
  saveBudgets: (v: { currency: Currency; rows: { kindId: number; amount: number | null }[] }) => invoke<void>("fin_budgets_save", v),
  totals: () => invoke<Total[]>("fin_totals"),
};

export const FIN_KEY = ["fin"];
export const useAccounts = () => useQuery({ queryKey: [...FIN_KEY, "accounts"], queryFn: api.accounts });
export const useAssets = () => useQuery({ queryKey: [...FIN_KEY, "assets"], queryFn: api.assets });
export const useHoldings = () => useQuery({ queryKey: [...FIN_KEY, "holdings"], queryFn: api.holdings });
export const useEntries = (month: string) => useQuery({ queryKey: [...FIN_KEY, "entries", month], queryFn: () => api.entries(month) });
export const useFixed = () => useQuery({ queryKey: [...FIN_KEY, "fixed"], queryFn: api.fixed });
export const useBudgets = () => useQuery({ queryKey: [...FIN_KEY, "budgets"], queryFn: api.budgets });
export const useTotals = () => useQuery({ queryKey: [...FIN_KEY, "totals"], queryFn: api.totals });

export function useFinMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  // Any change can move sums, balances and 分类 counts → refresh every finance query and the kinds.
  const onSuccess = () => {
    void qc.invalidateQueries({ queryKey: FIN_KEY });
    void qc.invalidateQueries({ queryKey: KINDS_KEY });
  };
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    createAccount: useMutation({ mutationFn: api.createAccount, onSuccess }),
    updateAccount: useMutation({ mutationFn: api.updateAccount, onSuccess }),
    deleteAccount: useMutation({ mutationFn: api.deleteAccount, onSuccess, onError }),
    reorderAccounts: useMutation({ mutationFn: api.reorderAccounts, onSuccess, onError }),
    saveMonth: useMutation({ mutationFn: api.saveMonth, onSuccess }),
    saveHoldings: useMutation({ mutationFn: api.saveHoldings, onSuccess }),
    createEntry: useMutation({ mutationFn: api.createEntry, onSuccess }),
    createEntries: useMutation({ mutationFn: api.createEntries, onSuccess }),
    updateEntry: useMutation({ mutationFn: api.updateEntry, onSuccess }),
    deleteEntry: useMutation({ mutationFn: api.deleteEntry, onSuccess, onError }),
    createFixed: useMutation({ mutationFn: api.createFixed, onSuccess }),
    updateFixed: useMutation({ mutationFn: api.updateFixed, onSuccess }),
    deleteFixed: useMutation({ mutationFn: api.deleteFixed, onSuccess, onError }),
    reorderFixed: useMutation({ mutationFn: api.reorderFixed, onSuccess, onError }),
    saveBudgets: useMutation({ mutationFn: api.saveBudgets, onSuccess }),
  };
}
