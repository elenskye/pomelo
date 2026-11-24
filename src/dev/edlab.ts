// 写字的框的测试台（docs/testing.md「写字的框」）：在真页面里（所有样式都在）用三种写法（见字如面 bear.ts、念念不忘 lite.ts、
// 写提示词 markdown.ts）把一组样本各画一遍：没在写、光标停在每一行（那一行的记号露出来）、光标走过每一个字，两种宽度。查：
//   字没叠在一起（每段字的位置两两不重叠；一个盒子装得下它自己的字）    style   只被它自己的样式表画（别的模块的规则不碰框里的东西）
//   text    读回来的字和原文一样；光标放到第 i 个字、读回来还是 i     paint   只改变了的行（LiveText）和整个重画一样
//   hit     能点的（折叠箭头、「…」、待办的框、链接、标签）点得到：它在最上面
// `?mock&edlab=1`：无头 Chrome（scripts/editor-lab.mjs 读 window.__edlab）；`&edlab=<端口>`：Safari（WebKit），结果 POST 回来。
import { drawDoc, type Look } from "@/modules/articles/bear";
import { renderCode, renderMath } from "@/modules/articles/render";
import { offsetOf, paint, select, selectionIn, serialize, lineStartsOf } from "@/shared/editor/editable";
import { liteFlavor } from "@/shared/editor/lite";
import { markdownFlavor } from "@/shared/editor/markdown";
import type { Flavor } from "@/shared/editor/LiveText";
import { showGroups } from "@/shared/editor/editable";
import "@/shared/editor/MdMenu"; // hands the icon drawings to the editors

type Fail = { check: string; where: string; detail: string };
export type LabResult = { done: boolean; agent: string; ms: number; states: number; checks: Record<string, number>; fails: Fail[] };

const PIC = "data:image/svg+xml;utf8," + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="160" height="90"><rect width="160" height="90" fill="#cde"/></svg>');
const LOOK: Look = {
  image: (name) => (name === "pic.png" ? PIC : null),
  file: (name) => name === "pic.png" || name === "notes.pdf",
  thing: (t) =>
    t === "不存在的一篇"
      ? null
      : t.startsWith("念念不忘/")
        ? { module: "notes", id: 7, title: "今天的云很好看", meta: null, date: "2026-11-01 10:00", text: "今天的云很好看，**像棉花糖**。", locked: false, target: t }
        : { module: "articles", id: 3, title: t, meta: null, date: "2026-11-02 09:00", text: null, locked: false, target: t },
  sticker: () => "",
  math: renderMath,
  mermaid: () => ({ svg: '<svg xmlns="http://www.w3.org/2000/svg" width="220" height="60"><rect width="220" height="60" fill="#eee"/></svg>' }),
  code: renderCode,
  folds: new Set(),
};

// made-up text covering each construct (and the one that broke: inline code before CJK, in a paragraph and a table)
const BEAR = [
  "# 写作测试 `code` 标题",
  "#测试/编辑器 #另一个",
  "",
  "依据 `docs/design.md`，分三类。中英混排 `ab` 后面紧跟中文，`很长很长的一段代码 with spaces and more` 再接着写。",
  "**加粗** *斜体* ~下划线~ ~~删除线~~ ==高亮== ==🟢绿色== ==🔴红== `代码` 和 [链接](https://example.com) 以及 <https://example.org>。",
  "嵌套：**粗体里有 `代码` 和 ==高亮==**，*斜体里的 [链接](https://a.b)*。",
  "柚子链接 [[东京的第三天]]、[[东京的第三天|换个说法]]、[[不存在的一篇]]，标签 #旅行/东京，图标 :pm-star: 在字里。",
  "脚注[^1]，公式 $E = mc^2$ 和 $\\sum_i x_i$ 在行内。",
  "一个很长的网址 https://example.com/a/very/long/path/that/should/wrap/somewhere/in/the/line?with=query&and=more 结尾。",
  "Supercalifragilisticexpialidocious_and_an_even_longer_unbroken_identifier_that_must_wrap_somewhere 结尾。",
  "",
  "## 列表",
  "- 第一项 `x`",
  "- 第二项",
  "  - 嵌套一层 **粗**",
  "    - 再嵌套",
  "1. 编号一",
  "10. 编号十",
  "- [ ] 没做完 `todo`",
  "- [x] 做完了",
  "",
  "### 引用和提示框",
  "> 一段引用，里面有 `代码` 和 **加粗**。",
  "> 第二行",
  "",
  "> [!TIP]",
  "> 提示框的正文。",
  "",
  "> [!WARNING] 带标题的提示框",
  "> 正文 `x`",
  "",
  '### 折着的标题 <!-- {"fold":true} -->',
  "被折起来的内容。",
  "",
  "#### 表格",
  "| 功能 | 循环 | 命令行 |",
  "| --- | --- | --- |",
  "| 一句话一个任务，前后各拍一份快照 | ✓ | ✓ |",
  "| 回退（`restore` / `undo` snapshot） | `a` | 中文 |",
  "| 很长的一格：同一项目一次只跑一个任务，不同项目能同时跑 |  | ✓ |",
  "",
  "```ts",
  "const x: number = 1; // 注释",
  "function f(a: string) { return a.length; }",
  "```",
  "",
  "$$",
  "\\int_0^1 x^2 \\, dx = \\frac{1}{3}",
  "$$",
  "",
  "```mermaid",
  "flowchart LR",
  "  A --> B",
  "```",
  "",
  "![](pic.png)",
  "行内图片 ![](pic.png) 后面的字，附件 [notes.pdf](notes.pdf)。",
  "![[念念不忘/2026-11-01 10:00]]",
  "",
  "---",
  "",
  "##### 五级标题",
  "###### 六级标题",
  "",
  "[^1]: 脚注的内容，`代码` 也行。",
].join("\n");

const LITE = [
  "今天 **加粗** ==高亮== ++下划线++ 和 #标签/子标签，网址 https://example.com/x 结尾。",
  "",
  "- 列表一 #旅行",
  "- 列表二 **粗**",
  "1. 编号",
  "图标 :pm-star: 在字里，柚子链接 [[见字如面/东京的第三天]]。",
  "很长的英文 Supercalifragilisticexpialidocious_and_an_even_longer_unbroken_identifier_wrap 结尾。",
].join("\n");

const MD = [
  "# 标题 `代码`",
  "写提示词：**加粗** *斜体* `code` 和 [链接](https://example.com)。",
  "",
  "- 列表 `x`",
  "1. 编号",
  "> 引用",
  "```",
  "代码块 line",
  "```",
  "{{名称}} 和 :pm-star: 图标。",
].join("\n");

type Case = { name: string; flavor: Flavor; text: string; box: (w: number) => { host: HTMLElement; el: HTMLElement }; css: RegExp; widths: number[] };
const ALLOWED_COMMON = /\/src\/(index\.css|vendor\/pomelo\/(ds|pomelo)\.css|shared\/links\/links\.css|shared\/editor\/md-menu\.css)$|katex/;

function shell(html: string, w: number) {
  const host = document.createElement("div");
  host.style.cssText = `position:fixed;left:0;top:0;width:${w}px;height:100vh;overflow:auto;z-index:99999;background:var(--background)`;
  host.innerHTML = html;
  document.body.appendChild(host);
  return { host, el: host.querySelector<HTMLElement>("[contenteditable]")! };
}
const ROOT = 'role="textbox" aria-multiline="true" contenteditable="plaintext-only" spellcheck="false"';
// 见字如面's box as Editor.tsx builds it, in the look 设置 gives it (size, line height, space after a paragraph)
const bearBox = (size: number, leading: number, para: number) => (w: number) =>
  shell(`<div class="ar-editor" style="--ar-size:${size}px;--ar-leading:${leading};--ar-para:${para}em;--ar-width:none"><div class="ar-scroll pm-scroll"><div class="ar-page"><div class="relative"><div ${ROOT} class="block w-full leading-relaxed break-words whitespace-pre-wrap outline-none ar-text"></div></div></div></div></div>`, w);
const BEAR_FLAVOR: Flavor = { caretAware: true, draw: (t, p, a, s) => drawDoc(t, p, a, LOOK, s), onCaret: showGroups };
const ARTICLES_CSS = /\/src\/modules\/articles\/articles\.css$/;
const CASES: Case[] = [
  { name: "见字如面", flavor: BEAR_FLAVOR, text: BEAR, box: bearBox(17, 1.75, 0.5), css: ARTICLES_CSS, widths: [360, 720] },
  { name: "见字如面（大字、宽行距）", flavor: BEAR_FLAVOR, text: BEAR, box: bearBox(21, 2.1, 1), css: ARTICLES_CSS, widths: [560] },
  {
    name: "念念不忘",
    flavor: liteFlavor({ tags: true }),
    text: LITE,
    box: (w) => shell(`<div class="text-[15px]"><div class="relative"><div ${ROOT} class="block w-full leading-relaxed break-words whitespace-pre-wrap outline-none"></div></div></div>`, w),
    css: /^$/,
    widths: [360, 720],
  },
  {
    name: "写提示词",
    flavor: markdownFlavor(),
    text: MD,
    box: (w) => shell(`<div class="pm-field pm-compact pm-area flex w-full cursor-text text-sm"><div class="-mx-2.5 -my-1.5 min-h-0 flex-1 px-3 py-1.5"><div class="relative"><div ${ROOT} class="block w-full leading-relaxed break-words whitespace-pre-wrap outline-none"></div></div></div></div>`, w),
    css: /^$/,
    widths: [360, 720],
  },
];

// ---------- the checks ----------

const transparent = (c: string) => /rgba?\([^)]*,\s*0(\.0+)?\)$/.test(c) || c === "transparent";
/** Every visible piece of text in the box: its rect (cut to the boxes that clip it) and its node. */
function textRects(root: HTMLElement) {
  const out: { r: DOMRect; n: Text }[] = [];
  const range = document.createRange();
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = walk.nextNode() as Text | null; n; n = walk.nextNode() as Text | null) {
    if (!n.data.trim()) continue;
    let hidden = false;
    let clip = { left: -Infinity, top: -Infinity, right: Infinity, bottom: Infinity };
    for (let e: HTMLElement | null = n.parentElement; e && e !== root.parentElement; e = e.parentElement) {
      const cs = getComputedStyle(e);
      if (cs.display === "none" || cs.visibility === "hidden" || Number(cs.opacity) === 0 || (e === n.parentElement && (transparent(cs.color) || parseFloat(cs.fontSize) === 0))) {
        hidden = true;
        break;
      }
      if (cs.overflowX !== "visible" || cs.overflowY !== "visible" || cs.clipPath !== "none") {
        const b = e.getBoundingClientRect();
        clip = { left: Math.max(clip.left, b.left), top: Math.max(clip.top, b.top), right: Math.min(clip.right, b.right), bottom: Math.min(clip.bottom, b.bottom) };
      }
    }
    if (hidden) continue;
    range.selectNodeContents(n);
    for (const r of range.getClientRects()) {
      const c = new DOMRect(Math.max(r.left, clip.left), Math.max(r.top, clip.top), 0, 0);
      c.width = Math.min(r.right, clip.right) - c.left;
      c.height = Math.min(r.bottom, clip.bottom) - c.top;
      if (c.width > 0.5 && c.height > 0.5) out.push({ r: c, n });
    }
  }
  return out;
}

const said = (n: Node) => JSON.stringify((n.textContent ?? "").slice(0, 24));
function overlaps(root: HTMLElement): string[] {
  const rs = textRects(root).sort((a, b) => a.r.top - b.r.top);
  const bad: string[] = [];
  for (let i = 0; i < rs.length; i++)
    for (let j = i + 1; j < rs.length && rs[j].r.top < rs[i].r.bottom; j++) {
      const a = rs[i];
      const b = rs[j];
      if (a.n === b.n) continue;
      const w = Math.min(a.r.right, b.r.right) - Math.max(a.r.left, b.r.left);
      const h = Math.min(a.r.bottom, b.r.bottom) - Math.max(a.r.top, b.r.top);
      if (w > 1.5 && h > 0.4 * Math.min(a.r.height, b.r.height)) bad.push(`${said(a.n)} 和 ${said(b.n)} 叠在一起（${Math.round(w)}×${Math.round(h)}）`);
    }
  return bad;
}
/** A box sized by its own rules (inline-block / grid / flex) that its own words stick out of. */
function outOfBox(root: HTMLElement): string[] {
  const bad: string[] = [];
  const range = document.createRange();
  for (const e of root.querySelectorAll<HTMLElement>("*")) {
    const cs = getComputedStyle(e);
    if (!/^inline-(block|grid|flex)$/.test(cs.display) || cs.overflowX !== "visible" || e.closest("[data-pv]")) continue;
    const b = e.getBoundingClientRect();
    if (!b.width) continue;
    for (const t of e.childNodes) {
      if (t.nodeType !== Node.TEXT_NODE || !t.textContent?.trim()) continue;
      range.selectNodeContents(t);
      const r = range.getBoundingClientRect();
      if (r.width && (r.left < b.left - 1 || r.right > b.right + 1)) bad.push(`.${[...e.classList].join(".")} 宽 ${Math.round(b.width)}，里面的字 ${said(t)} 宽 ${Math.round(r.width)}`);
    }
  }
  return bad;
}

type RuleRef = { sel: string; file: string };
let rulesByClass: Map<string, RuleRef[]> | null = null;
function ruleIndex() {
  if (rulesByClass) return rulesByClass;
  const m = new Map<string, RuleRef[]>();
  const add = (rules: CSSRuleList, file: string) => {
    for (const r of Array.from(rules)) {
      if (r instanceof CSSStyleRule) {
        for (const part of r.selectorText.split(/,(?![^(]*\))/)) {
          const key = part.trim().split(/\s*[\s>+~]\s*/).pop() ?? "";
          for (const c of key.matchAll(/\.((?:\\.|[\w-])+)/g)) {
            const k = c[1].replace(/\\(.)/g, "$1");
            (m.get(k) ?? m.set(k, []).get(k)!).push({ sel: part.trim(), file });
          }
        }
      } else if ("cssRules" in r) add((r as CSSGroupingRule).cssRules, file);
    }
  };
  for (const s of Array.from(document.styleSheets)) {
    const own = s.ownerNode as HTMLElement | null;
    const file = own?.dataset?.viteDevId ?? s.href ?? "inline";
    try {
      add(s.cssRules, file);
    } catch {
      /* a sheet from elsewhere */
    }
  }
  return (rulesByClass = m);
}
/** Rules from other modules' stylesheets that reach into the box. */
function strangers(root: HTMLElement, allowed: RegExp): string[] {
  const idx = ruleIndex();
  const bad = new Set<string>();
  for (const e of [root, ...root.querySelectorAll<HTMLElement>("*")])
    for (const c of e.classList)
      for (const r of idx.get(c) ?? []) {
        if (ALLOWED_COMMON.test(r.file) || allowed.test(r.file)) continue;
        try {
          if (e.matches(r.sel)) bad.add(`${r.file.replace(/^.*\/src\//, "src/")} 的「${r.sel}」画到了框里的 .${c}`);
        } catch {
          /* a selector this browser doesn't take */
        }
      }
  return [...bad];
}
/** What a click lands on: each clickable bit must be the topmost thing at its middle. */
function unreachable(root: HTMLElement, host: HTMLElement): string[] {
  const bad: string[] = [];
  for (const e of root.querySelectorAll<HTMLElement>("[data-fold], .folded, [data-task], [data-link], [data-href], [data-file], .pm-hashtag")) {
    // a link that wraps: its first piece
    const r0 = e.getClientRects()[0];
    if (!r0 || !r0.width || !r0.height) continue;
    host.scrollTop += r0.top - host.getBoundingClientRect().top - 100;
    const r = e.getClientRects()[0];
    const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    if (!top || (top !== e && !e.contains(top))) bad.push(`点 .${[...e.classList].join(".") || e.tagName} ${said(e)} 落在 .${top ? [...top.classList].join(".") || top.tagName : "（无）"} 上`);
  }
  return bad;
}

// ---------- the run ----------

export async function runEdLab(port: string): Promise<LabResult> {
  await new Promise((r) => setTimeout(r, 800));
  const t0 = performance.now();
  const res: LabResult = { done: false, agent: navigator.userAgent, ms: 0, states: 0, checks: {}, fails: [] };
  const count = (k: string) => (res.checks[k] = (res.checks[k] ?? 0) + 1);
  const fail = (check: string, where: string, detail: string) => {
    if (res.fails.length < 200 && !res.fails.some((f) => f.check === check && f.detail === detail)) res.fails.push({ check, where, detail });
  };
  for (const c of CASES)
    for (const w of c.widths) {
      const { host, el } = c.box(w);
      const text = c.text;
      const lines = text.split("\n");
      const starts = lineStartsOf(text);
      let drawn: string[] = [];
      const show = (active: number | null, sel: [number, number] | null, label: string, pending: number | null = null) => {
        const next = c.flavor.draw(text, pending, active, sel);
        // drawing from what was there (only the changed lines) comes out as drawing it whole
        if (drawn.length) {
          paint(el, drawn, next);
          const whole = document.createElement("div");
          whole.innerHTML = next.join("");
          count("paint");
          if (el.innerHTML !== whole.innerHTML) {
            const k = [...el.children].findIndex((ch, i) => ch.outerHTML !== whole.children[i]?.outerHTML);
            const a = el.children[k]?.outerHTML ?? "";
            const b = whole.children[k]?.outerHTML ?? "";
            const at = [...a].findIndex((ch, i) => ch !== b[i]);
            fail("paint", label, `只重画变了的行，和整个重画的不一样：第 ${k + 1} 行 ${JSON.stringify(a.slice(Math.max(0, at - 30), at + 50))} ≠ ${JSON.stringify(b.slice(Math.max(0, at - 30), at + 50))}`);
          }
        } else el.innerHTML = next.join("");
        drawn = next;
        res.states++;
        count("text");
        const back = serialize(el);
        if (back !== text) {
          const i = [...back].findIndex((ch, k) => ch !== [...text][k]);
          fail("text", label, `读回来的字不一样（第 ${i} 个字起：${JSON.stringify(back.slice(i, i + 20))}，原文 ${JSON.stringify(text.slice(i, i + 20))}）`);
        }
        count("layout");
        for (const d of [...overlaps(el), ...outOfBox(el)]) fail("layout", label, d);
      };
      const where = (l: number | null) => `${c.name} · 宽 ${w} · ${l === null ? "没在写" : `光标在第 ${l + 1} 行「${lines[l].slice(0, 16)}」`}`;
      show(null, null, where(null));
      count("style");
      for (const d of strangers(el, c.css)) fail("style", where(null), d);
      count("hit");
      for (const d of unreachable(el, host)) fail("hit", where(null), d);
      for (let l = 0; l < lines.length; l++) {
        const end = starts[l] + lines[l].length;
        show(l, [end, end], where(l));
        // a blank line just made by ↩ is drawn as the line about to be written (LiveText `pending`): its own height, nothing over it
        if (!lines[l].trim()) show(l, [end, end], `${where(l)}（刚回车）`, l);
        if (l % 6 === 0) {
          count("style");
          for (const d of strangers(el, c.css)) fail("style", where(l), d);
        }
        // the caret through the line: the marks it's in show (onCaret), and it lands where it was put — or, inside a
        // unit it can't enter (a heading's marks in the margin, an icon in the words), at one of its ends
        const units = [...el.querySelectorAll<HTMLElement>('[contenteditable="false"]:not([data-pv])')].map((u) => {
          const a = offsetOf(el, u, 0, starts);
          return [a, a + serialize(u).length];
        });
        for (let p = starts[l]; p <= end; p++) {
          c.flavor.onCaret?.(el, [p, p], starts);
          select(el, p, p, starts);
          const got = selectionIn(el, starts);
          count("caret");
          const unit = units.find(([a, b]) => p > a && p < b);
          if (unit && got && got[0] === got[1] && (got[0] === unit[0] || got[0] === unit[1])) continue;
          if (!got || got[0] !== p || got[1] !== p) fail("caret", where(l), `光标放到第 ${p} 个字，读回来是 ${JSON.stringify(got)}（「${text.slice(Math.max(starts[l], p - 6), p)}|${text.slice(p, p + 6)}」）`);
        }
      }
      host.remove();
    }
  res.ms = Math.round(performance.now() - t0);
  res.done = true;
  (window as unknown as { __edlab?: LabResult }).__edlab = res;
  document.title = `edlab ${res.fails.length ? "FAIL" : "ok"}`;
  if (/^\d+$/.test(port) && port !== "1") await fetch(`http://127.0.0.1:${port}/`, { method: "POST", mode: "no-cors", body: JSON.stringify(res) }).catch(() => {});
  return res;
}
