// 见字如面's print page (print.html in the hidden window `print`, made by print.rs for one export). Asks Rust for the
// job, then for each article: PDF — lays out the pages (layout.ts), shows them one at a time at the top-left while
// Rust has WebKit draw each, then hands over the links and outline; Word — sends the blocks, formulas and diagrams
// drawn as pictures first. Light, the palette chosen in 设置. A failed article is reported and the next one goes on.
// Browser-only look (`pnpm dev`): /print.html?mock&id=<article>&style=pomelo|print[&toc&header&meta][&word][&repeat=n]
import { invoke } from "@tauri-apps/api/core";
import "@/index.css";
import "@/modules/articles/articles.css";
import "@/vendor/pomelo/handmade.js";
import "./print.css";
import { DEFAULT_APPEARANCE, isAppearance } from "@/shared/appearance";
import { layout, longDate, themeOf, wordBlocks, type Item, type Options, type Snap } from "./layout";

const params = new URLSearchParams(location.search);
const host = document.getElementById("pages")!;
const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

async function palette() {
  const root = document.documentElement;
  const a = await invoke<unknown>("settings_get", { key: "appearance" }).catch(() => null);
  root.dataset.palette = isAppearance(a) ? a.palette : DEFAULT_APPEARANCE.palette;
  root.dataset.theme = "light";
  root.style.colorScheme = "light";
  // the Design System's colours (--color-*) follow the palette inside an element marked light
  host.dataset.theme = "light";
}

type JobView = { id: number; items: Item[]; options: Options };

async function pdf(job: number, index: number, item: Item, o: Options) {
  const laid = await layout(item, o, host);
  host.classList.add("pv-live");
  for (const p of laid.pages) {
    for (const q of laid.pages) q.classList.toggle("on", q === p);
    await invoke("print_page", { job });
  }
  await invoke("print_pdf_save", { job, index, links: laid.links, outline: laid.outline });
  host.classList.remove("pv-live");
}

async function word(job: number, index: number, item: Item, o: Options) {
  const snap: Snap = async (box) => {
    const s = document.createElement("div");
    s.className = `pv-snap ar-text pv-text pv-${o.style}`;
    s.dataset.theme = "light";
    // on 柚儿园's paper colour (Word's page is that colour too)
    if (o.style === "pomelo") s.style.background = "var(--color-paper)";
    s.append(box.cloneNode(true));
    document.body.append(s);
    host.style.visibility = "hidden";
    try {
      const r = s.getBoundingClientRect();
      if (r.width < 1 || r.height < 1) return null;
      const pic = await invoke<number>("print_picture", { job, w: Math.ceil(r.width), h: Math.ceil(r.height) });
      return { pic, w: r.width, h: r.height };
    } finally {
      s.remove();
      host.style.visibility = "";
    }
  };
  const blocks = await wordBlocks(item, o, host, snap);
  const doc = { title: item.title || "无标题", date: o.meta ? longDate(item.createdAt) : null, tags: o.meta ? item.tags : [], blocks, theme: themeOf(o.style, host) };
  await invoke("print_docx_save", { job, index, doc });
}

async function run() {
  const view = await invoke<JobView>("print_job");
  const job = view.id;
  await palette();
  await document.fonts.ready;
  for (const [i, item] of view.items.entries()) {
    try {
      if (view.options.format === "pdf") await pdf(job, i, item, view.options);
      else await word(job, i, item, view.options);
    } catch (e) {
      await invoke("print_fail", { job, index: i, error: errorText(e) }).catch(() => {});
    }
  }
  host.innerHTML = "";
  await invoke("print_done", { job });
}

/** The browser's look at one article's pages (or its Word blocks). */
async function preview() {
  await (await import("@/dev/mockIpc")).installMockIpc("print");
  await palette();
  document.body.classList.add("pv-preview");
  const id = Number(params.get("id") ?? 1);
  const a = await invoke<Item & { attachments: Item["files"] }>("article_get", { id });
  // &repeat=n: the body n more times (to see pages cut)
  const more = Number(params.get("repeat") ?? 0);
  const body = a.content.split("\n").slice(1).join("\n");
  const item: Item = { ...a, content: a.content + `\n\n## 再来一遍\n${body}`.repeat(more), files: a.attachments };
  const o: Options = { format: params.has("word") ? "docx" : "pdf", style: params.get("style") === "print" ? "print" : "pomelo", toc: params.has("toc"), header: params.has("header"), meta: params.has("meta") };
  if (o.format === "pdf") {
    const laid = await layout(item, o, host);
    (window as unknown as { __laid: unknown }).__laid = { pages: laid.pages.length, links: laid.links, outline: laid.outline };
  } else {
    const blocks = await wordBlocks(item, o, host, async (b) => ({ pic: 0, w: b.offsetWidth, h: b.offsetHeight }));
    const pre = document.createElement("pre");
    pre.style.cssText = "background:#fff;padding:12px;font:11px/1.4 Menlo,monospace;white-space:pre-wrap;width:595px";
    pre.textContent = JSON.stringify({ blocks, theme: themeOf(o.style, host) }, null, 1);
    host.replaceChildren(pre);
  }
}

if (import.meta.env.DEV && params.has("mock")) void preview();
else void run().catch((e) => console.error(e));
