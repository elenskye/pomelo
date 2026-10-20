// A plain-text editable box drawn one block (div) per line: the text is the model, the DOM is redrawn
// from it after each change and the selection put back by offsets in the text. Non-text bits inside a
// line (an image shown under it) count for nothing. Used by 念念不忘 and 呦呦漫游's editors.

/** The text of a box (or a cloned piece of it): blocks are lines; a `<br>` ending a block is only
 * its placeholder. */
export function serialize(root: Node): string {
  const lines: string[] = [];
  let cur: string | null = null;
  const inline = (n: Node): string => {
    if (n.nodeType === Node.TEXT_NODE) return n.textContent ?? "";
    if (n.nodeName === "BR") return n.nextSibling ? "\n" : "";
    return Array.from(n.childNodes).map(inline).join("");
  };
  for (const n of Array.from(root.childNodes)) {
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

/** Offset in the text of a DOM position inside `root`. */
export function offsetOf(root: HTMLElement, node: Node, offset: number): number {
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

export function selectionIn(root: HTMLElement): [number, number] | null {
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0 || !root.contains(sel.anchorNode) || !root.contains(sel.focusNode)) return null;
  const a = offsetOf(root, sel.anchorNode!, sel.anchorOffset);
  const f = offsetOf(root, sel.focusNode!, sel.focusOffset);
  return [Math.min(a, f), Math.max(a, f)];
}

/** The DOM position of a text offset (in a box drawn one block per line). */
export function pointAt(root: HTMLElement, pos: number): [Node, number] {
  const lines = Array.from(root.children);
  let p = pos;
  for (const line of lines) {
    const len = serialize(wrapOne(line)).length;
    if (p <= len) {
      const walker = document.createTreeWalker(line, NodeFilter.SHOW_TEXT);
      let t: Node | null;
      let last: Node | null = null;
      while ((t = walker.nextNode())) {
        const n = t.textContent?.length ?? 0;
        if (p <= n) return [t, p];
        p -= n;
        last = t;
      }
      return last ? [last, last.textContent?.length ?? 0] : [line, 0];
    }
    p -= len + 1;
  }
  return [root, root.childNodes.length];
}
const wrapOne = (n: Node) => {
  const f = document.createDocumentFragment();
  f.appendChild(n.cloneNode(true));
  return f;
};

export function select(root: HTMLElement, start: number, end = start) {
  const sel = window.getSelection();
  if (!sel) return;
  const [an, ao] = pointAt(root, start);
  const [fn, fo] = pointAt(root, end);
  sel.setBaseAndExtent(an, ao, fn, fo);
}

export const lineStart = (v: string, i: number) => v.lastIndexOf("\n", i - 1) + 1;
