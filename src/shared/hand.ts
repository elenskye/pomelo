// 手画的框、线、勾选框、开关、小图标（Design System 的 data-hand 写法）：同步来的 handmade.js（visual-atelier 的
// Design System「Pomelo 柚儿园」的 bundle.js，不手改）照元素上的 data-hand、data-seed… 画进元素里的一层 [data-hand-painted]。
// 整个窗口一个画手：元素出现、改了 data-hand 的参数、变了大小就补画（Pomelo.paint 只画变了的）。
// 颜色取 ds.css 的 --color-*（和 Design System、visual-atelier 的原型同一份变量）。样子和动静在 ds.css（index.css 引入）。
import "@/vendor/pomelo/handmade.js";

type PomeloNS = { paint: (root?: ParentNode, o?: object) => boolean | void };
declare global {
  interface Window {
    Pomelo?: PomeloNS;
  }
}

const WATCHED = ["data-hand", "data-seed", "data-fill", "data-tilt", "data-r", "data-sw", "data-amp", "data-size", "data-d", "data-o", "data-tapes", "data-stitches", "data-stroke", "data-pad"];

/** The white boxes on a module's paper (tables, lists, cards: `rounded-card border bg-card`) become the Design
 *  System's card — a hand-drawn edge, the typical pages of visual-atelier's canvases/pomelo-pages — without touching
 *  each page: marked data-auto-card + data-hand="card" here (frame.css styles them). Menus, dialogs, pills and boxes
 *  smaller than a row are left alone; a box that scrolls keeps its overflow (its edge stays inside). */
const AUTO = ".pm-sheet .rounded-card.border.bg-card:not([data-hand]):not([data-no-hand])";
let autoSeed = 2000;
function autoCards() {
  for (const el of document.querySelectorAll<HTMLElement>(AUTO)) {
    if (el.closest("dialog,[role=menu],[role=listbox],[role=dialog],.pm-menu,[popover],.pm-panel")) continue;
    const r = el.getBoundingClientRect();
    // a box smaller than a row is a pill or a badge; a one-line card (体检「没有问题」, a short list) is still a card
    if (r.width < 160 || r.height < 30) continue;
    const cs = getComputedStyle(el);
    if (cs.position === "static") el.style.position = "relative";
    const scrolls = /(auto|scroll)/.test(cs.overflowY);
    el.dataset.autoCard = "";
    if (scrolls) el.dataset.autoScroll = "";
    el.dataset.hand = "card";
    el.dataset.amp = "0.7";
    el.dataset.pad = scrolls ? "0" : "3";
    el.dataset.seed = String(autoSeed++);
  }
}

let started = false;
/** Once, before the first render: paints now and whenever the page changes. */
export function startHand() {
  if (started || typeof window === "undefined") return;
  started = true;
  const seen = new WeakSet<Element>();
  const ro = new ResizeObserver(() => schedule());
  let queued = false;
  const run = () => {
    queued = false;
    const ns = window.Pomelo;
    if (!ns) return;
    autoCards();
    ns.paint(document.body);
    for (const el of document.querySelectorAll("[data-hand]")) {
      if (seen.has(el)) continue;
      seen.add(el);
      ro.observe(el);
    }
  };
  // the painted layer itself is a change: the next run finds nothing to repaint and stops there
  const schedule = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(run);
  };
  new MutationObserver(schedule).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: WATCHED });
  void document.fonts?.ready.then(schedule);
  schedule();
}

/** Restart a one-shot animation class (果冻弹、啪地贴上…): take it off, read the layout, put it back; off again after `ms`. */
export function flash(el: Element | null | undefined, cls: string, ms?: number) {
  if (!el) return;
  el.classList.remove(cls);
  void (el as HTMLElement).offsetWidth;
  el.classList.add(cls);
  if (ms) {
    const t = Number((el as HTMLElement).dataset.flashT);
    if (t) clearTimeout(t);
    (el as HTMLElement).dataset.flashT = String(window.setTimeout(() => el.classList.remove(cls), ms));
  }
}

/** The system's 减弱动态效果 (the motion falls back to its final look by itself; for timers and JS-driven moves). */
export const reducedMotion = () => typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
