// A plain-text editable box drawn one block (div) per line: the text is the model, the DOM is redrawn
// from it after each change and the selection put back by offsets in the text. Non-text bits inside a
// line (an image shown under it) count for nothing; so does anything marked `data-pv` (a rendered preview: a
// formula, a diagram, a checkbox drawn in place of its source), text and all. Used by LiveText (the shared editor)
// and Seeker's points.
// Offsets are found line by line without cloning: `starts` (each line's offset in the text, when the caller knows the
// box shows that text) skips counting the lines above; without it they are read from the DOM.

const isPv = (n: Node) => n.nodeType === Node.ELEMENT_NODE && (n as Element).hasAttribute("data-pv");

/** The text inside one block (or any node), `<br>` ending it being only its placeholder. */
function inline(n: Node): string {
  if (n.nodeType === Node.TEXT_NODE) return n.textContent ?? "";
  if (n.nodeName === "BR") return n.nextSibling ? "\n" : "";
  if (isPv(n)) return "";
  let s = "";
  for (let c = n.firstChild; c; c = c.nextSibling) s += inline(c);
  return s;
}

/** The text of a box (or a cloned piece of it): blocks are lines; a `<br>` ending a block is only
 * its placeholder. */
export function serialize(root: Node): string {
  const lines: string[] = [];
  let cur: string | null = null;
  for (const n of Array.from(root.childNodes)) {
    if (isPv(n)) continue;
    if (n.nodeName === "DIV" || n.nodeName === "P") {
      if (cur !== null) lines.push(cur);
      cur = null;
      lines.push(inline(n));
    } else {
      cur = (cur ?? "") + inline(n);
    }
  }
  if (cur !== null) lines.push(cur);
  return lines.join("\n");
}

/** Every top-level node is a line (a div) or a rendered bit: lines and text lines match one to one. */
function lined(root: HTMLElement): boolean {
  for (let c = root.firstChild; c; c = c.nextSibling) if (c.nodeName !== "DIV" && !isPv(c)) return false;
  return true;
}

/** Each line's offset in a text. */
export function lineStartsOf(text: string): number[] {
  const out = [0];
  for (let i = text.indexOf("\n"); i >= 0; i = text.indexOf("\n", i + 1)) out.push(i + 1);
  return out;
}

/** Offset in the text of a DOM position inside `root`. */
export function offsetOf(root: HTMLElement, node: Node, offset: number, starts?: number[]): number {
  if (lined(root)) {
    // the line holding the position, and where it starts
    let line: Node | null = node;
    if (node === root) {
      line = root.childNodes[offset] ?? null;
      if (!line) return serialize(root).length;
    } else while (line && line.parentNode !== root) line = line.parentNode;
    if (line && !isPv(line)) {
      let index = 0;
      let before = 0;
      for (let c = root.firstChild; c && c !== line; c = c.nextSibling) {
        if (isPv(c)) continue;
        if (!starts) before += inline(c).length + 1;
        index++;
      }
      if (starts) before = starts[index] ?? before;
      if (node === root) return before;
      const r = document.createRange();
      r.setStart(line, 0);
      r.setEnd(node, offset);
      return before + inline(r.cloneContents()).length;
    }
  }
  if (node === root) {
    const kids = Array.from(root.childNodes);
    if (offset >= kids.length) return serialize(root).length;
    const before = document.createDocumentFragment();
    kids.slice(0, offset).forEach((k) => before.appendChild(k.cloneNode(true)));
    return offset === 0 ? 0 : serialize(before).length + 1;
  }
  const r = document.createRange();
  r.setStart(root, 0);
  r.setEnd(node, offset);
  return serialize(r.cloneContents()).length;
}

export function selectionIn(root: HTMLElement, starts?: number[]): [number, number] | null {
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0 || !root.contains(sel.anchorNode) || !root.contains(sel.focusNode)) return null;
  const a = offsetOf(root, sel.anchorNode!, sel.anchorOffset, starts);
  const f = sel.focusNode === sel.anchorNode && sel.focusOffset === sel.anchorOffset ? a : offsetOf(root, sel.focusNode!, sel.focusOffset, starts);
  return [Math.min(a, f), Math.max(a, f)];
}

/** The DOM position of a text offset (in a box drawn one block per line). */
export function pointAt(root: HTMLElement, pos: number, starts?: number[]): [Node, number] {
  const lines = Array.from(root.children).filter((l) => !l.hasAttribute("data-pv"));
  let p = pos;
  let k = 0;
  if (starts && starts.length === lines.length) {
    // the line by its start (binary search), then only that line is walked
    let lo = 0;
    let hi = starts.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (starts[mid] <= pos) lo = mid;
      else hi = mid - 1;
    }
    k = lo;
    p = pos - starts[lo];
  }
  for (; k < lines.length; k++) {
    const line = lines[k];
    const len = inline(line).length;
    if (p <= len) {
      const walker = document.createTreeWalker(line, NodeFilter.SHOW_TEXT);
      let t: Node | null;
      let last: Node | null = null;
      while ((t = walker.nextNode())) {
        if (t.parentElement?.closest("[data-pv]")) continue;
        const n = t.textContent?.length ?? 0;
        // a unit the caret can't enter (an icon keeping its source inside): before it or after it
        const unit = t.parentElement?.closest<HTMLElement>('[contenteditable="false"]');
        if (unit && line.contains(unit)) {
          if (p <= n) return [unit.parentNode!, Array.prototype.indexOf.call(unit.parentNode!.childNodes, unit) + (p === 0 ? 0 : 1)];
          p -= n;
          last = t;
          continue;
        }
        if (p <= n) return [t, p];
        p -= n;
        last = t;
      }
      const unit = last?.parentElement?.closest<HTMLElement>('[contenteditable="false"]');
      if (unit && line.contains(unit)) return [unit.parentNode!, Array.prototype.indexOf.call(unit.parentNode!.childNodes, unit) + 1];
      return last ? [last, last.textContent?.length ?? 0] : [line, 0];
    }
    p -= len + 1;
  }
  return [root, root.childNodes.length];
}

export function select(root: HTMLElement, start: number, end = start, starts?: number[]) {
  const sel = window.getSelection();
  if (!sel) return;
  const [an, ao] = pointAt(root, start, starts);
  const [fn, fo] = end === start ? [an, ao] : pointAt(root, end, starts);
  sel.setBaseAndExtent(an, ao, fn, fo);
}

export const lineStart = (v: string, i: number) => v.lastIndexOf("\n", i - 1) + 1;

// ---------- marks shown where the caret is (见字如面's way, after Bear 2) ----------

/** Where the selection's ends are in one line (offsets in it); none when the box isn't focused or it's another line. */
export type Ends = readonly number[];
export const NO_ENDS: Ends = [];

/** The ends of `sel` falling in line `i` (whose start is `start`, length `len`), for a caret-aware flavor's draw. */
export const endsIn = (sel: [number, number] | null | undefined, start: number, len: number): Ends =>
  sel ? [sel[0], sel[1]].filter((e) => e >= start && e <= start + len).map((e) => e - start) : NO_ENDS;

/** One inline construct with marks, [a, b] in its line: `on` (its marks show) while an end of the selection is in it or
 * right next to it; elsewhere it reads as what it means. No offsets in the HTML: typing before it mustn't change how its
 * line is drawn (LiveText keeps a line the browser typed). */
export const fxOpen = (a: number, b: number, ends: Ends) => `<span class="fx${ends.some((e) => e >= a && e <= b) ? " on" : ""}">`;
export const fxGroup = (a: number, b: number, ends: Ends, html: string) => `${fxOpen(a, b, ends)}${html}</span>`;

/** The caret moved within its line (a flavor's `onCaret`): the groups (`.fx`) holding an end of the selection show their
 * marks, the others hide them — the classes the draw would give, so the next draw finds the line as it would draw it.
 * One div per line. */
export function showGroups(box: HTMLElement, sel: [number, number], starts: number[]) {
  const want = new Set<Element>();
  for (const e of sel[0] === sel[1] ? [sel[0]] : sel) {
    let lo = 0;
    let hi = starts.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (starts[mid] <= e) lo = mid;
      else hi = mid - 1;
    }
    box.children[lo]?.querySelectorAll<HTMLElement>(".fx").forEach((g) => {
      if (e >= offsetOf(box, g, 0, starts) && e <= offsetOf(box, g, g.childNodes.length, starts)) want.add(g);
    });
  }
  box.querySelectorAll(".fx.on").forEach((g) => !want.has(g) && g.classList.remove("on"));
  want.forEach((g) => !g.classList.contains("on") && g.classList.add("on"));
}
