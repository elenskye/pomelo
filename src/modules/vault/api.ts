// Typed wrappers for Rust vault_* commands (src-tauri/src/modules/vault.rs) + React Query hooks.
// All rules (master password, Touch ID, 3 days, 5-minute idle lock, encryption) live in Rust; the list
// never carries passwords, and copying goes Rust → pasteboard directly.
import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";

export type VaultState = "new" | "locked" | "open";
export type Reason = "none" | "failed" | "expired" | "updated" | "first";
export type VaultStatus = { state: VaultState; touch: boolean; reason: Reason | null };

export type Fields = {
  name: string;
  link: string | null;
  username: string | null;
  account: string | null;
  password: string | null;
  note: string | null;
};
export type Entry = Fields & { id: number; createdAt: string; updatedAt: string };
export type Summary = {
  id: number;
  name: string;
  link: string | null;
  username: string | null;
  account: string | null;
  hasPassword: boolean;
  hasNote: boolean;
  updatedAt: string;
};
export type CopyField = "username" | "account" | "password" | "link";

/** Mirrors reason_text in vault.rs (shown on the lock screen). */
export const REASON: Record<Reason, string | null> = {
  none: null,
  failed: "Touch ID 没有通过，请输入主密码",
  expired: "已经 3 天没输入主密码了",
  updated: "App 更新后，第一次要输入主密码",
  first: null,
};
export const LOCKED = "芝麻开门已经锁上了";
/** Same as IDLE in vault.rs; the UI locks itself too so the page doesn't sit open. */
export const IDLE_MS = 5 * 60 * 1000;

export const vaultApi = {
  status: () => invoke<VaultStatus>("vault_status"),
  setup: (password: string) => invoke<VaultStatus>("vault_setup", { password }),
  unlock: (password: string) => invoke<VaultStatus>("vault_unlock", { password }),
  unlockTouch: () => invoke<VaultStatus>("vault_unlock_touch"),
  lock: () => invoke<void>("vault_lock"),
  /** Counts as use; false = locked. */
  touch: () => invoke<boolean>("vault_touch"),
  changePassword: (v: { old: string; new: string }) => invoke<VaultStatus>("vault_change_password", v),
  list: () => invoke<Summary[]>("vault_list"),
  get: (id: number) => invoke<Entry>("vault_get", { id }),
  create: (fields: Fields) => invoke<Entry>("vault_create", { fields }),
  update: (v: { id: number; fields: Fields }) => invoke<Entry>("vault_update", v),
  remove: (id: number) => invoke<void>("vault_delete", { id }),
  copy: (v: { id: number; field: CopyField }) => invoke<void>("vault_copy", v),
  generate: () => invoke<string>("vault_generate"),
};

export const VAULT_KEY = ["vault"];
export const VAULT_STATUS_KEY = [...VAULT_KEY, "status"];
export const useVaultStatus = () => useQuery({ queryKey: VAULT_STATUS_KEY, queryFn: vaultApi.status });
export const useVaultList = (enabled: boolean) => useQuery({ queryKey: [...VAULT_KEY, "list"], queryFn: vaultApi.list, enabled });
export const useVaultEntry = (id: number | null) =>
  useQuery({ queryKey: [...VAULT_KEY, "entry", id], queryFn: () => vaultApi.get(id!), enabled: id !== null, gcTime: 0 });

export function useVaultMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  const refresh = () => void qc.invalidateQueries({ queryKey: VAULT_KEY });
  const onError = (e: unknown) => {
    toast(errorText(e), "error");
    if (errorText(e) === LOCKED) refresh();
  };
  return {
    create: useMutation({ mutationFn: vaultApi.create, onSuccess: refresh }),
    update: useMutation({ mutationFn: vaultApi.update, onSuccess: refresh }),
    remove: useMutation({ mutationFn: vaultApi.remove, onSuccess: refresh, onError }),
    copy: useMutation({ mutationFn: vaultApi.copy, onError }),
    lock: useMutation({ mutationFn: vaultApi.lock, onSettled: refresh }),
  };
}
