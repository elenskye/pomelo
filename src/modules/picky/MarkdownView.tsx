// Renders a message's Markdown (markdown.ts) into its own container, replacing only the top-level blocks
// whose HTML changed: finished paragraphs stay put (and selectable) while a reply streams in. A caret
// sits at the end of the text while streaming. React never manages the children of this container.
import { useLayoutEffect, useRef } from "react";
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

export function MarkdownView(props: { source: string; streaming?: boolean; className?: string; body?: boolean }) {
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
    }
  }, [props.source, props.streaming]);
  return <div ref={ref} data-body={props.body ? "" : undefined} className={`md ${props.className ?? ""}`} />;
}
