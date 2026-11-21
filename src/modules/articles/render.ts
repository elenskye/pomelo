// 见字如面's renders, cached across articles: formulas (KaTeX), code (highlight.js, cut into lines), Mermaid diagrams.
// The writing box (Editor.tsx) and the print page (src/print) draw with the same ones.
import hljs from "highlight.js/lib/common";
import katex from "katex";
import "katex/dist/katex.min.css";
import { errorText } from "@/shared/feedback";

const mathCache = new Map<string, string | null>();
export function renderMath(tex: string, display: boolean): string | null {
  const k = `${display ? "D" : "I"}\u0000${tex}`;
  if (!mathCache.has(k)) {
    try {
      mathCache.set(k, katex.renderToString(tex, { displayMode: display, throwOnError: true, strict: "ignore", trust: false, maxSize: 20, maxExpand: 500 }));
    } catch {
      mathCache.set(k, null);
    }
  }
  return mathCache.get(k) ?? null;
}

const codeCache = new Map<string, string[] | null>();
/** hljs HTML cut into lines, every line closing and reopening the spans it is inside. */
function splitLines(html: string): string[] {
  const out: string[] = [];
  const open: string[] = [];
  let cur = "";
  for (const part of html.split(/(<span[^>]*>|<\/span>|\n)/)) {
    if (part === "\n") {
      out.push(cur + "</span>".repeat(open.length));
      cur = open.join("");
    } else if (part.startsWith("<span")) {
      open.push(part);
      cur += part;
    } else if (part === "</span>") {
      open.pop();
      cur += part;
    } else cur += part;
  }
  out.push(cur);
  return out;
}
export function renderCode(src: string, lang: string): string[] | null {
  const k = `${lang}\u0000${src}`;
  if (!codeCache.has(k)) {
    const l = lang && hljs.getLanguage(lang) ? lang : null;
    try {
      codeCache.set(k, l ? splitLines(hljs.highlight(src, { language: l, ignoreIllegals: true }).value) : null);
    } catch {
      codeCache.set(k, null);
    }
  }
  return codeCache.get(k) ?? null;
}

export type Diagram = { svg: string } | { error: string };
const diagramCache = new Map<string, Diagram>();
const diagramWaits = new Map<string, Promise<void>>();
let mermaidSeq = 0;
export function renderDiagram(src: string, dark: boolean, then: () => void): Diagram | null {
  const k = `${dark ? "d" : "l"}\u0000${src}`;
  const done = diagramCache.get(k);
  if (done) return done;
  if (!diagramWaits.has(k)) {
    const job = import("mermaid")
      .then(async ({ default: mermaid }) => {
        mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: dark ? "dark" : "neutral", fontFamily: "PingFang SC, sans-serif" });
        const { svg } = await mermaid.render(`pm-mmd-${++mermaidSeq}`, src);
        diagramCache.set(k, { svg });
      })
      .catch((e) => void diagramCache.set(k, { error: `画不出来：${errorText(e).split("\n")[0]}` }))
      .finally(then);
    diagramWaits.set(k, job);
  } else void diagramWaits.get(k)!.then(then);
  return null;
}


/** A diagram once drawn (for the print page, which waits for it). */
export function diagramReady(src: string, dark: boolean): Promise<Diagram> {
  return new Promise((done) => {
    const now = renderDiagram(src, dark, () => done(renderDiagram(src, dark, () => {}) ?? { error: "画不出来" }));
    if (now) done(now);
  });
}
