// Money and month helpers for 钱来钱往. Amounts are integer cents (Rust stores the same).
import { localToday, toYmd } from "@/shared/dates";

export type Currency = "CNY" | "SGD" | "HKD" | "USD";
export const CURRENCIES: Currency[] = ["CNY", "SGD", "HKD", "USD"];
export const CURRENCY_NAME: Record<Currency, string> = { CNY: "人民币", SGD: "新币", HKD: "港币", USD: "美元" };
const SYMBOL: Record<Currency, string> = { CNY: "¥", SGD: "S$", HKD: "HK$", USD: "US$" };

const grouped = (n: number, decimals: number) =>
  n.toLocaleString("en-US", { minimumFractionDigits: decimals, maximumFractionDigits: decimals });

/** 12345 → "¥123.45"; whole amounts drop ".00"; `sign` adds "+" to positives. */
export function money(cents: number, currency: Currency, sign = false): string {
  const v = Math.abs(cents) / 100;
  const body = `${SYMBOL[currency]}${grouped(v, cents % 100 === 0 ? 0 : 2)}`;
  return cents < 0 ? `-${body}` : sign && cents > 0 ? `+${body}` : body;
}

/** Axis ticks: "¥3,000", "¥1.2万". */
export function moneyShort(cents: number, currency: Currency): string {
  const v = cents / 100;
  if (Math.abs(v) >= 10000) return `${SYMBOL[currency]}${grouped(v / 10000, v % 10000 === 0 ? 0 : 1)}万`;
  return `${SYMBOL[currency]}${grouped(v, 0)}`;
}

/** "1,234.5" / "¥12" → cents; blank or invalid → null. */
export function parseMoney(text: string): number | null {
  const t = text.replace(/[,\s¥$]|S\$|HK\$|US\$/g, "");
  if (!/^-?\d+(\.\d{0,2})?$/.test(t)) return null;
  return Math.round(Number(t) * 100);
}

/** cents → input text ("123.45" / "120"). */
export const moneyInput = (cents: number | null | undefined) => (cents == null ? "" : String(cents % 100 === 0 ? cents / 100 : (cents / 100).toFixed(2)));

export const thisMonth = () => localToday().slice(0, 7);
export const shiftMonth = (ym: string, k: number) => toYmd(new Date(Number(ym.slice(0, 4)), Number(ym.slice(5, 7)) - 1 + k, 1)).slice(0, 7);
export const monthTitle = (ym: string) => `${Number(ym.slice(0, 4))} 年 ${Number(ym.slice(5, 7))} 月`;
/** Chart label: "10月", or "26/1" at a year start. */
export const monthTick = (ym: string, first: boolean) => (first || ym.endsWith("-01") ? `${ym.slice(2, 4)}/${Number(ym.slice(5))}` : `${Number(ym.slice(5))}月`);

/** Mirrors finance.rs FixedRule::on_or_after (prefills 下次日期 in the form). */
export function nextOnOrAfter(rule: string, day: string): string {
  const [kind, arg = ""] = rule.split(":");
  let [y, m] = [Number(day.slice(0, 4)), Number(day.slice(5, 7))];
  for (let k = 0; k < 30; k++) {
    const last = new Date(y, m, 0).getDate();
    let d: number | null = null;
    if (kind === "monthly") d = Math.min(Number(arg), last);
    else if (kind === "yearly" && Number(arg.slice(0, 2)) === m) d = Math.min(Number(arg.slice(3)), last);
    const ymd = d === null ? null : `${y}-${String(m).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
    if (ymd && ymd >= day) return ymd;
    [y, m] = m === 12 ? [y + 1, 1] : [y, m + 1];
  }
  return day;
}

/** "每月 15 号" / "每年 3 月 1 日". */
export function ruleText(rule: string): string {
  const [kind, arg = ""] = rule.split(":");
  return kind === "monthly" ? `每月 ${arg} 号` : `每年 ${Number(arg.slice(0, 2))} 月 ${Number(arg.slice(3))} 日`;
}
