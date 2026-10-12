// Markdown → HTML for 小拾 messages (ported from the old Picky's Resources/Web/markdown.js, same rules).
// - Raw HTML from the model is escaped; only http/https links become anchors; images are never loaded
//   (a link-like label instead); tables get a horizontal scroller; fenced code gets a language label,
//   a copy button and highlighting — except a fence still open while streaming.
// - Math: $$…$$ and \[…\] display, \(…\) and $…$ inline. A single $ opens only before a non-space and
//   closes only after a non-space with no digit next, so "$5 and $10" stays text. Unclosed formulas
//   (still streaming) stay source text; formulas KaTeX can't parse show their source, faded. Rendered
//   formulas carry data-source (with delimiters) and data-tex for copying.
import MarkdownIt from "markdown-it";
import hljs from "highlight.js/lib/common";
import katex from "katex";

/* eslint-disable @typescript-eslint/no-explicit-any -- markdown-it rule internals */
type Any = any;

const COPY_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="8.5" y="8.5" width="11.5" height="11.5" rx="2.5"/><path d="M15.5 8.5V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7.5a2 2 0 0 0 2 2h2.5"/></svg>';
const EXTERNAL_ICON =
  '<svg class="ext" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 16 16 8M9.5 8H16v6.5"/></svg>';

const md = new MarkdownIt({ html: false, linkify: true, breaks: true, typographer: false });
// file names such as README.md must not become links
md.linkify.set({ fuzzyLink: false, fuzzyEmail: false, fuzzyIP: false });
const escapeHtml = md.utils.escapeHtml;
const isWebURL = (href: string | null) => /^https?:\/\//i.test(href ?? "");

const CACHE_LIMIT = 300;
const highlightCache = new Map<string, string>();
const mathCache = new Map<string, string>();
function cached(cache: Map<string, string>, key: string, make: () => string) {
  const hit = cache.get(key);
  if (hit !== undefined) {
    cache.delete(key);
    cache.set(key, hit);
    return hit;
  }
  const value = make();
  cache.set(key, value);
  if (cache.size > CACHE_LIMIT) cache.delete(cache.keys().next().value!);
  return value;
}

function highlight(code: string, lang: string) {
  if (!lang || !hljs.getLanguage(lang)) return escapeHtml(code);
  return cached(highlightCache, `${lang}\u0000${code}`, () => {
    try {
      return hljs.highlight(code, { language: lang, ignoreIllegals: true }).value;
    } catch {
      return escapeHtml(code);
    }
  });
}

// ---------- math ----------

// With throwOnError, KaTeX only uses errorColor for untrusted commands (\href, \url…); the sentinel
// turns those into the same faded source as a parse error.
const ERROR_SENTINEL = "#0ff1ce";
const KATEX_OPTIONS = { throwOnError: true, strict: "ignore" as const, trust: false, maxSize: 20, maxExpand: 500, errorColor: ERROR_SENTINEL };

function renderMath(tex: string, source: string, display: boolean) {
  return cached(mathCache, `${display ? "D" : "I"}\u0000${source}`, () => {
    const tag = display ? "div" : "span";
    const attrs = ` data-tex="${escapeHtml(tex)}" data-source="${escapeHtml(source)}"`;
    let html: string | null = null;
    if (tex.trim()) {
      try {
        html = katex.renderToString(tex, { displayMode: display, ...KATEX_OPTIONS });
        if (html.includes(ERROR_SENTINEL)) html = null;
      } catch {
        html = null;
      }
    }
    if (html === null) return `<${tag} class="math-error" title="公式无法解析"${attrs}>${escapeHtml(source)}</${tag}>`;
    return `<${tag} class="${display ? "math-display" : "math"}"${attrs}>${html}</${tag}>`;
  });
}

const isSpace = (c: number) => c === 0x20 || c === 0x09 || c === 0x0a || c === 0x0d;
const isDigit = (c: number) => c >= 0x30 && c <= 0x39;
function escaped(src: string, pos: number) {
  let n = 0;
  while (pos - 1 - n >= 0 && src.charCodeAt(pos - 1 - n) === 0x5c) n++;
  return n % 2 === 1;
}
/** Index of an unescaped `close` at or after `from`, or -1. */
function findClose(src: string, close: string, from: number, max: number) {
  let at = from;
  for (;;) {
    at = src.indexOf(close, at);
    if (at === -1 || at + close.length > max) return -1;
    if (!escaped(src, at)) return at;
    at += 1;
  }
}

function mathInline(state: Any, silent: boolean) {
  const src: string = state.src;
  const pos: number = state.pos;
  let max: number = state.posMax;
  let open: string, close: string, display: boolean;
  if (src.charCodeAt(pos) === 0x5c) {
    const next = src.charAt(pos + 1);
    if ((next !== "(" && next !== "[") || escaped(src, pos)) return false;
    open = "\\" + next;
    close = next === "(" ? "\\)" : "\\]";
    display = next === "[";
  } else if (src.charCodeAt(pos) === 0x24) {
    if (escaped(src, pos)) return false;
    if (src.charCodeAt(pos + 1) === 0x24) {
      open = "$$";
      close = "$$";
      display = true;
    } else {
      open = "$";
      close = "$";
      display = false;
      if (pos + 1 >= max || isSpace(src.charCodeAt(pos + 1))) return false;
      // a single-$ formula stays on its line and never spans a code span
      const lineEnd = src.indexOf("\n", pos);
      if (lineEnd !== -1 && lineEnd < max) max = lineEnd;
      const tick = src.indexOf("`", pos);
      if (tick !== -1 && tick < max) max = tick;
    }
  } else return false;
  const start = pos + open.length;
  let end = -1;
  let search = start;
  while (search < max) {
    const at = findClose(src, close, search, max);
    if (at === -1) break;
    if (open === "$") {
      if (at === start || isSpace(src.charCodeAt(at - 1)) || (at + 1 < max && isDigit(src.charCodeAt(at + 1)))) {
        search = at + 1;
        continue;
      }
    } else if (at === start) {
      search = at + close.length;
      continue;
    }
    end = at;
    break;
  }
  if (end === -1) {
    // unclosed (often still streaming): keep the opening delimiter as written
    if (open === "$") return false;
    if (!silent) state.pending += open;
    state.pos = start;
    return true;
  }
  if (!silent) {
    const token = state.push(display ? "math_inline_display" : "math_inline", "math", 0);
    token.content = src.slice(start, end);
    token.markup = open;
    token.info = src.slice(pos, end + close.length);
  }
  state.pos = end + close.length;
  return true;
}

/** $$ or \[ starting a line, closed by $$ or \] ending a line; a blank line first = not (yet) a formula. */
function mathBlock(state: Any, startLine: number, endLine: number, silent: boolean) {
  if (state.sCount[startLine] - state.blkIndent >= 4) return false;
  const start = state.bMarks[startLine] + state.tShift[startLine];
  const first: string = state.src.slice(start, state.eMarks[startLine]);
  const open = first.slice(0, 2);
  let close: string;
  if (open === "$$") close = "$$";
  else if (open === "\\[") close = "\\]";
  else return false;
  const closes = (text: string) => text.length >= close.length && text.slice(-close.length) === close && !escaped(text, text.length - close.length);
  const rest = first.slice(2).trim();
  let content: string;
  let line = startLine;
  if (rest.length > close.length && closes(rest)) content = rest.slice(0, -close.length);
  else if (rest === close) return false;
  else {
    const lines = rest ? [rest] : [];
    let found = false;
    for (line = startLine + 1; line < endLine; line++) {
      if (state.isEmpty(line)) break;
      const text = (state.src.slice(state.bMarks[line], state.eMarks[line]) as string).replace(/\s+$/, "");
      if (closes(text)) {
        lines.push(text.slice(0, -close.length));
        found = true;
        break;
      }
      lines.push(text);
    }
    if (!found) return false;
    content = lines.join("\n");
  }
  if (!content.trim()) return false;
  if (silent) return true;
  const token = state.push("math_block", "math", 0);
  token.block = true;
  token.content = content;
  token.markup = open;
  token.info = state.src.slice(start, state.eMarks[line]).trim();
  token.map = [startLine, line + 1];
  state.line = line + 1;
  return true;
}

md.inline.ruler.before("escape", "math_inline", mathInline);
md.block.ruler.before("fence", "math_block", mathBlock, { alt: ["paragraph", "reference", "blockquote", "list"] });

function codeBlock(code: string, lang: string, plain: boolean) {
  const label = lang ? escapeHtml(lang) : "代码";
  const classes = "hljs" + (lang ? ` language-${escapeHtml(lang)}` : "");
  return `<div class="code-block"><div class="code-head"><span>${label}</span><button type="button" data-action="copy-code" title="复制代码">${COPY_ICON}<span>复制</span></button></div><pre><code class="${classes}">${plain ? escapeHtml(code) : highlight(code, lang)}</code></pre></div>\n`;
}

const rules = md.renderer.rules;
rules.fence = (tokens: Any, idx: number, _o: Any, env: Any) => {
  const token = tokens[idx];
  const info = token.info ? md.utils.unescapeAll(token.info).trim() : "";
  return codeBlock(token.content, info.split(/\s+/)[0] || "", env.openFence === idx);
};
rules.code_block = (tokens: Any, idx: number) => codeBlock(tokens[idx].content, "", false);
rules.math_inline = (tokens: Any, idx: number) => renderMath(tokens[idx].content, tokens[idx].info, false);
rules.math_inline_display = (tokens: Any, idx: number) =>
  renderMath(tokens[idx].content, tokens[idx].info, true).replace(/^<div/, "<span").replace(/<\/div>$/, "</span>");
rules.math_block = (tokens: Any, idx: number) => renderMath(tokens[idx].content, tokens[idx].info, true) + "\n";
rules.table_open = () => '<div class="table-scroll"><table>\n';
rules.table_close = () => "</table></div>\n";
rules.link_open = (tokens: Any, idx: number, options: Any, env: Any, self: Any) => {
  const token = tokens[idx];
  const href = token.attrGet("href");
  env.linkStack = env.linkStack || [];
  if (!isWebURL(href)) {
    env.linkStack.push(false);
    return '<span class="link-text">';
  }
  env.linkStack.push(true);
  token.attrSet("title", href);
  return self.renderToken(tokens, idx, options);
};
rules.link_close = (_t: Any, _i: number, _o: Any, env: Any) => (env.linkStack && env.linkStack.pop() ? EXTERNAL_ICON + "</a>" : "</span>");
rules.image = (tokens: Any, idx: number, options: Any, env: Any, self: Any) => {
  const token = tokens[idx];
  const src = token.attrGet("src");
  const alt = self.renderInlineAsText(token.children || [], options, env) || "图片";
  const label = "图片：" + escapeHtml(alt);
  if (isWebURL(src) && !(env.linkStack && env.linkStack.length)) return `<a class="img-link" href="${escapeHtml(src)}" title="${escapeHtml(src)}">${label}</a>`;
  return `<span class="img-link">${label}</span>`;
};

/** True when the source ends inside an unclosed ``` or ~~~ fence (CommonMark rules, top level only). */
export function endsInsideFence(source: string) {
  let open: { char: string; length: number } | null = null;
  for (const line of source.split("\n")) {
    const m = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(line);
    if (!m) continue;
    const marker = m[1];
    if (!open) {
      if (marker[0] === "`" && m[2].includes("`")) continue;
      open = { char: marker[0], length: marker.length };
    } else if (marker[0] === open.char && marker.length >= open.length && m[2].trim() === "") open = null;
  }
  return open !== null;
}

export function renderMarkdown(source: string, streaming = false) {
  const text = source || "";
  const env: Any = {};
  const tokens = md.parse(text, env);
  if (streaming && endsInsideFence(text)) {
    for (let i = tokens.length - 1; i >= 0; i--) {
      if (tokens[i].type === "fence") {
        env.openFence = i;
        break;
      }
    }
  }
  return md.renderer.render(tokens, md.options, env);
}
