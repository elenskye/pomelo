// An icon written in a text: `:pm-<key>:` is one of Pomelo's own icons (the icon library, shared/iconLibrary.tsx),
// drawn in place wherever Pomelo shows that text — the live editors (lite.ts, markdown.ts), a note, a title.
// (`:name:` without `pm-` is the website's content icon: only the website draws it, here it stays as written.)
// This file knows only the notation; the library hands in the drawings (`setIconDrawer`), so lite.ts stays plain
// enough to run in Node.
export const ICON_TOKEN = /:pm-([a-z0-9-]+):/g;
export const iconToken = (key: string) => `:pm-${key}:`;

let drawer: ((key: string) => string | null) | null = null;
/** The inside of an icon's SVG (24 grid, one stroke) by key; null when there is no such icon. */
export const setIconDrawer = (f: (key: string) => string | null) => void (drawer = f);

/** Icon tokens in a line: [start, end, key]. */
export function iconSpans(line: string): [number, number, string][] {
  return [...line.matchAll(ICON_TOKEN)].map((m) => [m.index, m.index + m[0].length, m[1]]);
}

/** The token as an editor draws it: the icon, with its source kept inside (hidden) so the box's text is unchanged;
 *  one unit for the caret (contenteditable=false: the arrows step over it, ⌫ takes it whole). null: no such icon. */
export function iconHtml(key: string): string | null {
  const body = drawer?.(key);
  if (!body) return null;
  return `<span class="md-ico" contenteditable="false"><svg viewBox="0 0 24 24" fill="none" aria-hidden="true">${body}</svg><span class="md-ico-src">${iconToken(key)}</span></span>`;
}

/** A text without its icon tokens (one-line previews). */
export const withoutIcons = (text: string) => text.replace(ICON_TOKEN, "");
