// Local-date helpers. Dates are "YYYY-MM-DD" strings in the Mac's local time (Rust uses the same).
const pad = (n: number) => String(n).padStart(2, "0");

export const toYmd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
export const localToday = () => toYmd(new Date());

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

const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
export const weekday = (s: string) => WEEKDAYS[parseYmd(s).getDay()];

/** "2026-10-01" → "10 月 1 日"（not this year: "2025 年 10 月 1 日"）. */
export function formatDay(d: string | null): string | null {
  if (!d) return null;
  const [y, m, day] = d.split("-").map(Number);
  const md = `${m} 月 ${day} 日`;
  return y === new Date().getFullYear() ? md : `${y} 年 ${md}`;
}

/** "2026-10-03" → "2026 / 10 / 03". */
export const slashDate = (d: string) => d.split("-").join(" / ");
