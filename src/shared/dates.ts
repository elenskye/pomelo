// Local-date helpers. Dates are "YYYY-MM-DD" strings in the Mac's local time (Rust uses the same).
export const pad = (n: number) => String(n).padStart(2, "0");

export const toYmd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
export const localToday = () => toYmd(new Date());
/** "2026-10" moved by `k` months. */
export const shiftMonth = (ym: string, k: number) => toYmd(new Date(Number(ym.slice(0, 4)), Number(ym.slice(5, 7)) - 1 + k, 1)).slice(0, 7);
/** Today for the frame (顶栏, 柚子瓣): "10 月 3 日 · 周五". */
export const todayText = () => {
  const d = new Date();
  return `${d.getMonth() + 1} 月 ${d.getDate()} 日 · 周${"日一二三四五六"[d.getDay()]}`;
};

export function parseYmd(s: string): Date {
  const [y, m, d] = s.split("-").map(Number);
  return new Date(y, m - 1, d);
}

export function addDays(s: string, n: number): string {
  const d = parseYmd(s);
  d.setDate(d.getDate() + n);
  return toYmd(d);
}

/** Monday of the week containing `s`. */
export function mondayOf(s: string): string {
  const day = parseYmd(s).getDay(); // 0 = Sunday
  return addDays(s, day === 0 ? -6 : 1 - day);
}

/** Week heads, Monday first. */
export const WEEK = ["一", "二", "三", "四", "五", "六", "日"];

const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
export const weekday = (s: string) => WEEKDAYS[parseYmd(s).getDay()];

/** "2026-10-01" → "10 月 1 日"（not this year: "2025 年 10 月 1 日"）. */
export function formatDay(d: string | null): string | null {
  if (!d) return null;
  const [y, m, day] = d.split("-").map(Number);
  const md = `${m} 月 ${day} 日`;
  return y === new Date().getFullYear() ? md : `${y} 年 ${md}`;
}

/** "2026-10-07" → "10/7". */
export const monthDay = (d: string) => `${Number(d.slice(5, 7))}/${Number(d.slice(8))}`;

/** "2026-10-03" → "2026 / 10 / 03". */
export const slashDate = (d: string) => d.split("-").join(" / ");

/** A stored timestamp (UTC) → local "2026 / 02 / 23". */
export const stampDay = (s: string) => {
  const d = new Date(s);
  return `${d.getFullYear()} / ${pad(d.getMonth() + 1)} / ${pad(d.getDate())}`;
};

/** A stored timestamp (UTC) → local "2026 / 02 / 23 14:39". */
export function stampTime(stamp: string): string {
  const d = new Date(stamp);
  return `${d.getFullYear()} / ${pad(d.getMonth() + 1)} / ${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
