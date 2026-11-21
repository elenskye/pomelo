// 流畅度自测（docs/testing.md「流畅度」）：只在 `pnpm dev` 的 `?mock&perf=<端口>` 里跑。
// 自己把各页点一遍（先逐个打开，再在开着的页之间换），每次量 1.5 秒里帧和帧之间的间隔，再把最长的那块滚一遍；
// 结果 POST 给本机 `scripts/perf-listen.mjs`（Safari 和 Pomelo 的窗口是同一个 WebKit，Chrome 量不出纸纹这类只在 WebKit 里慢的东西）。
// `&perfcss=<样式>` 先塞一段样式再量，对比用。

const ROUTES = ["/todo", "/finance", "/notes", "/books", "/jobs", "/days", "/bookmarks", "/youzi", "/calendar", "/settings"];

type Sample = { what: string; first: number; worst: number; /** when the worst frame began, ms after the act */ at: number; jank: number; frames: number; hand: number };

/** Time spent painting the hand-drawn layers (shared/hand.ts → Pomelo.paint), per sample. */
let handMs = 0;
function timeHand() {
  const ns = (window as unknown as { Pomelo?: { paint: (...a: unknown[]) => unknown } }).Pomelo;
  if (!ns) return;
  const paint = ns.paint;
  ns.paint = (...a) => {
    const t = performance.now();
    try {
      return paint(...a);
    } finally {
      handMs += performance.now() - t;
    }
  };
}

const frame = () => new Promise<number>((r) => requestAnimationFrame(r));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Frame gaps over `ms`: `first` is how long the first frame after `act` took to come, `worst` the longest gap,
 *  `jank` the time lost to frames over 25 ms. */
async function watch(what: string, ms: number, act: () => void): Promise<Sample> {
  await frame();
  const t0 = performance.now();
  handMs = 0;
  act();
  let prev = t0, first = 0, worst = 0, at = 0, jank = 0, frames = 0;
  while (prev - t0 < ms) {
    const t = await frame();
    const now = Math.max(t, performance.now());
    const gap = now - prev;
    if (!frames) first = gap;
    if (gap > worst) {
      worst = gap;
      at = prev - t0;
    }
    if (gap > 25) jank += gap - 16.7;
    frames++;
    prev = now;
  }
  return { what, first: Math.round(first), worst: Math.round(worst), at: Math.round(at), jank: Math.round(jank), frames, hand: Math.round(handMs) };
}

function scroller(): HTMLElement | null {
  let best: HTMLElement | null = null;
  for (const el of document.querySelectorAll<HTMLElement>(".pm-page.front *, .deskpage, .deskpage *")) {
    if (el.scrollHeight - el.clientHeight < 60) continue;
    if (!/(auto|scroll)/.test(getComputedStyle(el).overflowY)) continue;
    if (!best || el.scrollHeight * el.clientWidth > best.scrollHeight * best.clientWidth) best = el;
  }
  return best;
}

async function scrollRun(what: string): Promise<Sample | null> {
  const el = scroller();
  if (!el) return null;
  let on = true;
  const s = watch(what, 1000, () => {
    const step = () => {
      if (!on) return;
      el.scrollTop += 24;
      requestAnimationFrame(step);
    };
    step();
  });
  const r = await s;
  on = false;
  el.scrollTop = 0;
  return r;
}

export async function runPerf(port: string) {
  const css = new URLSearchParams(location.search).get("perfcss");
  if (css) document.head.appendChild(Object.assign(document.createElement("style"), { textContent: css }));
  timeHand();
  await sleep(2500);
  const out: Sample[] = [];
  const go = (to: string) => () => (location.hash = `#${to}`);
  for (const r of ROUTES) {
    out.push(await watch(`open ${r}`, 1500, go(r)));
    const s = await scrollRun(`scroll ${r}`);
    if (s) out.push(s);
  }
  for (const r of ROUTES) out.push(await watch(`switch ${r}`, 1200, go(r)));
  out.push(await watch("home", 1500, go("/")));
  const s = await scrollRun("scroll home");
  if (s) out.push(s);
  const body = JSON.stringify({ ua: navigator.userAgent, css: css ?? "", w: innerWidth, h: innerHeight, dpr: devicePixelRatio, hands: document.querySelectorAll("[data-hand]").length, out });
  await fetch(`http://127.0.0.1:${port}/`, { method: "POST", mode: "no-cors", body }).catch(() => {});
  document.title = "perf done";
}
