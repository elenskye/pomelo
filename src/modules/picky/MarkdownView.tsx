// Renders a message's Markdown (markdown.ts) into its own container, replacing only the top-level blocks
// whose HTML changed: finished paragraphs stay put (and selectable) while a reply streams in. A caret
// sits at the end of the text while streaming. React never manages the children of this container.
// A finished reply links the names of records its lookups returned (`links`): the first mention of
// each becomes a .record-link (Transcript opens it like 全局搜索).
import { useLayoutEffect, useRef } from "react";
import type { QueryRecord } from "./api";
import { renderMarkdown } from "./markdown";
import "katex/dist/katex.min.css";
import "./messages.css";

type Blocks = { nodes: Element[]; keys: string[] };

function renderBlocks(box: HTMLElement, state: Blocks, html: string) {
  const template = document.createElement("template");
  template.innerHTML = html;
  const next: Element[] = [];
  template.content.childNodes.forEach((n) => {
    if (n.nodeType === Node.ELEMENT_NODE) next.push(n as Element);
    else if (n.nodeType === Node.TEXT_NODE && n.textContent?.trim()) {
      const p = document.createElement("p");
      p.textContent = n.textContent;
      next.push(p);
    }
  });
  const keys = next.map((n) => n.outerHTML);
  const nodes = state.nodes;
  for (let i = 0; i < next.length; i++) {
    if (i < nodes.length) {
      if (state.keys[i] === keys[i]) continue;
      box.replaceChild(next[i], nodes[i]);
      nodes[i] = next[i];
    } else {
      box.appendChild(next[i]);
      nodes.push(next[i]);
    }
  }
  while (nodes.length > next.length) nodes.pop()!.remove();
  state.keys = keys;
}

/** Where the caret goes: the innermost last text block (into code, never into a table / rule). */
function caretHost(node: Element): Element | null {
  for (;;) {
    if (node.classList.contains("code-block")) return node.querySelector("code");
    if (node.classList.contains("table-scroll") || node.tagName === "HR") return null;
    const last = node.lastElementChild;
    if (last && /^(UL|OL|LI|BLOCKQUOTE)$/.test(node.tagName) && /^(LI|P|UL|OL|BLOCKQUOTE|DIV|H[1-6])$/.test(last.tagName)) {
      node = last;
      continue;
    }
    return node;
  }
}

/** Kinds whose title is a real name (收支 titles are notes or categories; notes are excerpts). */
const NAMED = new Set(["group", "goal", "task", "book", "film", "course", "lesson", "trip", "template", "account", "fixed", "job"]);
const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
/** "甲公司 · 产品经理" also matches "甲公司·产品经理" / "甲公司 产品经理"; spaces are optional. */
const pattern = (title: string) =>
  title
    .split(/\s*[·・]\s*/)
    .map((part) => part.trim().split(/\s+/).map(escape).join("\\s*"))
    .join("\\s*[·・]?\\s*");

function linkRecords(box: HTMLElement, records: QueryRecord[]) {
  const done = new Set([...box.querySelectorAll<HTMLElement>(".record-link")].map((x) => x.dataset.key));
  // full names first, then a job's company alone; longer names win
  const wanted = records
    .filter((r) => NAMED.has(r.kind))
    .flatMap((r) => {
      const key = `${r.module}:${r.kind}:${r.id}`;
      const names = [r.title.trim()];
      if (r.kind === "job" && r.title.includes(" · ")) names.push(r.title.split(" · ")[0].trim());
      return names.filter((n) => [...n].length >= 2).map((n, i) => ({ key, record: r, re: new RegExp(pattern(n)), rank: i * 1000 - n.length }));
    })
    .filter((w) => !done.has(w.key))
    .sort((a, b) => a.rank - b.rank);
  if (!wanted.length) return;
  const walker = document.createTreeWalker(box, NodeFilter.SHOW_TEXT, {
    acceptNode: (n) => (n.parentElement?.closest("code, pre, a, .katex, .record-link") ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
  });
  const texts: Text[] = [];
  for (let n = walker.nextNode(); n; n = walker.nextNode()) texts.push(n as Text);
  for (const w of wanted) {
    if (done.has(w.key)) continue;
    for (let i = 0; i < texts.length; i++) {
      const t = texts[i];
      const m = w.re.exec(t.data);
      if (!m) continue;
      const hit = t.splitText(m.index);
      const rest = hit.splitText(m[0].length);
      const link = document.createElement("span");
      link.className = "record-link";
      link.setAttribute("role", "link");
      link.tabIndex = 0;
      link.dataset.key = w.key;
      link.dataset.record = JSON.stringify(w.record);
      hit.replaceWith(link);
      link.appendChild(hit);
      texts.splice(i, 1, t, rest); // the linked part is out; the rest can hold other names
      done.add(w.key);
      break;
    }
  }
}

export function MarkdownView(props: { source: string; streaming?: boolean; className?: string; body?: boolean; links?: QueryRecord[] }) {
  const ref = useRef<HTMLDivElement>(null);
  const blocks = useRef<Blocks>({ nodes: [], keys: [] });
  useLayoutEffect(() => {
    const box = ref.current;
    if (!box) return;
    box.querySelectorAll(".caret").forEach((c) => c.remove());
    renderBlocks(box, blocks.current, renderMarkdown(props.source, props.streaming));
    if (props.streaming) {
      const caret = document.createElement("span");
      caret.className = "caret";
      caret.setAttribute("aria-hidden", "true");
      const last = blocks.current.nodes[blocks.current.nodes.length - 1];
      ((last && caretHost(last)) || box).appendChild(caret);
    } else if (props.links?.length) {
      linkRecords(box, props.links);
    }
  }, [props.source, props.streaming, props.links]);
  return <div ref={ref} data-body={props.body ? "" : undefined} className={`md ${props.className ?? ""}`} />;
}
