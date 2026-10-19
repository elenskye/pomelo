// Typed wrappers for Rust days_* commands (src-tauri/src/modules/days.rs) + React Query hooks.
// Every rule (next occurrence, days, 周年 / 岁, soon, order, one pinned) is derived in Rust.
import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";

export type Kind = "day" | "birthday" | "document";
export type Repeat = "none" | "year" | "month" | "week";
export type Gift = { year: string; gift: string };
export type Day = {
  id: number;
  kind: Kind;
  name: string;
  /** YYYY-MM-DD; a birthday without the year: --MM-DD. */
  date: string;
  repeat: Repeat;
  /** Lead days, largest first (0 = 当天). */
  remind: number[];
  pinned: boolean;
  note: string | null;
  ideas: string | null;
  /** Newest year first. */
  gifts: Gift[];
  createdAt: string;
  updatedAt: string;
  /** The occurrence counted to. */
  next: string;
  /** next − today: ≥ 0 还有, < 0 已经 / 已过期. */
  days: number;
  /** 第几周年 / 几岁. */
  nth: number | null;
  soon: boolean;
  today: boolean;
};
export type DayPatch = Partial<{ kind: Kind; name: string; date: string; repeat: Repeat; remind: number[]; pinned: boolean; note: string | null; ideas: string | null; gifts: Gift[] }>;

export const KIND_LABEL: Record<Kind, string> = { day: "纪念日", birthday: "生日", document: "证件" };
export const REPEAT_LABEL: Record<Repeat, string> = { none: "不重复", year: "每年", month: "每月", week: "每周" };
/** Same as MAX_REMIND in days.rs. */
export const MAX_REMIND = 5;
export const DEFAULT_REMIND: Record<Kind, number[]> = { day: [7], birthday: [7], document: [30, 7] };

const api = {
  list: () => invoke<Day[]>("days_list"),
  create: (patch: DayPatch) => invoke<Day>("day_create", { patch }),
  update: (v: { id: number; patch: DayPatch }) => invoke<Day>("day_update", v),
  remove: (id: number) => invoke<void>("day_delete", { id }),
};

export const DAYS_KEY = ["days"];
export const useDays = () => useQuery({ queryKey: [...DAYS_KEY, "list"], queryFn: api.list });

export function useDayMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  const onSuccess = () => void qc.invalidateQueries({ queryKey: DAYS_KEY });
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    create: useMutation({ mutationFn: api.create, onSuccess }),
    update: useMutation({ mutationFn: api.update, onSuccess }),
    remove: useMutation({ mutationFn: api.remove, onSuccess, onError }),
  };
}

/** 「还有 7 天」「今天」「已经 225 天」「已过期 3 天」: [before, number, after]; number null = 今天. */
export function countText(d: Pick<Day, "kind" | "days">): [string, number | null, string] {
  if (d.days === 0) return ["今天", null, ""];
  if (d.days > 0) return ["还有", d.days, "天"];
  return [d.kind === "document" ? "已过期" : "已经", -d.days, "天"];
}

export const nthText = (d: Pick<Day, "kind" | "nth">) => (d.nth === null ? null : d.kind === "birthday" ? `${d.nth} 岁` : `第 ${d.nth} 周年`);
