// Template text helpers (from Picky's TemplateSlots / SlashMenuState): `{{名称}}` places to fill, the
// "/" menu's query and ordering, and the one-line summary on the empty page.
import type { Template } from "./api";

export type Slot = { start: number; end: number; name: string };

/** `{{名称}}`: 1–40 characters without braces or line breaks, not just spaces. */
export function slots(text: string): Slot[] {
  const out: Slot[] = [];
  for (const m of text.matchAll(/\{\{([^{}\n]{1,40})\}\}/g)) {
    const name = m[1].trim();
    if (name) out.push({ start: m.index, end: m.index + m[0].length, name });
  }
  return out;
}

/** The slot Tab (Shift-Tab) moves to from the selection, wrapping around. */
export function nextSlot(all: Slot[], selStart: number, selEnd: number, backwards: boolean): Slot | null {
  if (!all.length) return null;
  if (backwards) return [...all].reverse().find((s) => s.end <= selStart) ?? all[all.length - 1];
  return all.find((s) => s.start > selStart || (selStart === selEnd && s.start === selStart)) ?? all[0];
}

/** Text with each `{{名称}}` shown as its name. */
export function preview(text: string): string {
  let out = text;
  for (const s of slots(text).reverse()) out = out.slice(0, s.start) + s.name + out.slice(s.end);
  return out;
}

/** First non-empty line, places shown by name (the empty page's cards). */
export function summary(t: Template): string {
  const line = preview(t.body)
    .split("\n")
    .find((l) => l.trim());
  return (line ?? "").trim().replace(/^[\s：:]+|[\s：:]+$/g, "");
}

/** The "/" menu opens while the whole draft is "/" plus a filter without spaces or slashes, so
 *  "/usr/bin" or "/ 开头的句子" stay plain text. Returns the filter, or null. */
/** The "/word" just before the caret (the "/" opens a word: not after a letter, digit, ":" — a path, a
 * url, a date like 10/12); null when there is none. */
export function slashAt(text: string, caret: number): { start: number; end: number; query: string } | null {
  const before = text.slice(0, caret);
  const i = before.lastIndexOf("/");
  if (i < 0) return null;
  const query = before.slice(i + 1);
  if (/[\s/]/.test(query)) return null;
  if (i > 0 && /[A-Za-z0-9:./\\_-]/.test(before[i - 1])) return null;
  return { start: i, end: caret, query };
}

/** Matches (title or category): pinned ones first, then the rest by last use. */
export function slashMatches(all: Template[], query: string): { pinned: Template[]; others: Template[] } {
  const q = query.trim().toLowerCase();
  const found = all.filter((t) => !q || t.title.toLowerCase().includes(q) || t.category.toLowerCase().includes(q));
  const others = found
    .filter((t) => !t.pinned)
    .sort((a, b) => (b.lastUsedAt ?? "").localeCompare(a.lastUsedAt ?? "") || a.title.localeCompare(b.title, "zh"));
  return { pinned: found.filter((t) => t.pinned), others };
}

/** The list grouped for 管理: 置顶, then each category, 未分类 last. */
export function templateGroups(all: Template[]): { title: string; items: Template[] }[] {
  const out: { title: string; items: Template[] }[] = [];
  const pinned = all.filter((t) => t.pinned);
  if (pinned.length) out.push({ title: "置顶", items: pinned });
  const rest = all.filter((t) => !t.pinned);
  for (const c of [...new Set(rest.map((t) => t.category).filter(Boolean))].sort()) out.push({ title: c, items: rest.filter((t) => t.category === c) });
  const none = rest.filter((t) => !t.category);
  if (none.length) out.push({ title: "未分类", items: none });
  return out;
}
