// 月历 (src-tauri/src/calendar.rs → calendar_span): 一步一步、来日方长、出门走走、投石问路 on one calendar,
// read-only. Rules live in Rust (see calendar.rs); hidden modules are left out. The page comes in v0.9's
// Dawn Plan · Phase 6; the 首页 desk gets this month's entries inside home_overview.
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import type { Hit } from "@/shared/open";

export type CalendarEntry = Hit & {
  /** Local date (YYYY-MM-DD). */
  day: string;
  source: "todo" | "day" | "trip" | "job";
  /** todo: due | next (a later occurrence) | done; day: day | birthday | document; trip: depart; job: active | got. */
  state: "due" | "next" | "done" | "day" | "birthday" | "document" | "depart" | "active" | "got";
  /** 第 3 次, 11/03 起, 30 岁, 第 2 周年, 证件到期, the job's status… */
  note: string | null;
};

const CALENDAR_KEY = ["calendar"];

const calendarSpan = (from: string, to: string, hidden: string[]) => invoke<CalendarEntry[]>("calendar_span", { from, to, hidden });

export const useCalendar = (from: string, to: string, hidden: string[]) =>
  useQuery({ queryKey: [...CALENDAR_KEY, from, to, hidden], queryFn: () => calendarSpan(from, to, hidden), refetchOnMount: "always" });
