// 落签 (the tooltip hanging on a string, Design System Tooltip: an element with `pm-tip-host` and a `.pm-tip` inside)
// is shown in a layer above everything, so no page, card or scrolling box can cut it off: while a host is pointed at
// (or has keyboard focus) a copy of its tip hangs from a fixed-positioned stand-in at the host's place, in the page —
// or in the open dialog, which sits above the page — and the tip inside the host stays hidden (index.css
// [data-tip-up]; hidden, not removed, so nothing around it moves). The copy swings with the Design System's own keyframes; near the window's edge the tag slides in
// along its string. A host given `is-hover` by code (柚子瓣's typed match) still shows its own tip in place.
let host: HTMLElement | null = null;
let float: HTMLElement | null = null;
let watch = 0;
let at = { x: 0, y: 0 };
const EDGE = 8;

function hide() {
  host?.removeAttribute("data-tip-up");
  float?.remove();
  host = float = null;
  window.clearInterval(watch);
}

const wanted = (h: HTMLElement) => h.isConnected && h.matches(":hover, :focus-visible, :has(> :focus-visible)");

function show(h: HTMLElement) {
  if (h === host) return;
  hide();
  const tip = h.querySelector<HTMLElement>(":scope > .pm-tip");
  if (!tip) return;
  const r = h.getBoundingClientRect();
  const top = parseFloat(getComputedStyle(tip).top);
  const stand = document.createElement("span");
  stand.className = "pm-tip-host is-hover tip-float";
  stand.setAttribute("aria-hidden", "true");
  stand.style.cssText = `position:fixed;left:${r.left}px;top:${r.top}px;width:${r.width}px;height:${r.height}px;`;
  const copy = tip.cloneNode(true) as HTMLElement;
  copy.removeAttribute("role");
  copy.style.top = `${Number.isFinite(top) ? top : r.height + 4}px`;
  stand.appendChild(copy);
  (h.closest("dialog[open]") ?? document.body).appendChild(stand);
  h.setAttribute("data-tip-up", "");
  host = h;
  float = stand;
  at = { x: r.left, y: r.top };
  // the tag stays inside the window (its string keeps hanging from the middle of the host)
  const tag = copy.querySelector<HTMLElement>(".pm-tip-tag");
  if (tag) {
    const w = tag.offsetWidth;
    const mid = r.left + r.width / 2;
    const dx = Math.min(0, window.innerWidth - EDGE - (mid + w / 2)) + Math.max(0, EDGE - (mid - w / 2));
    if (dx) tag.style.translate = `${dx}px 0`;
  }
  watch = window.setInterval(() => host && !wanted(host) && hide(), 200);
}

const hostOf = (t: EventTarget | null) => (t instanceof Element ? t.closest<HTMLElement>(".pm-tip-host:not(.tip-float)") : null);

let started = false;
/** Once per window. */
export function startTips() {
  if (started || typeof document === "undefined") return;
  started = true;
  const over = (e: Event) => {
    const h = hostOf(e.target);
    if (h) show(h);
    else if (host && !wanted(host)) hide();
  };
  document.addEventListener("pointerover", over, true);
  document.addEventListener("mouseover", over, true);
  document.addEventListener("focusin", (e) => {
    const h = hostOf(e.target);
    if (h && wanted(h)) show(h);
  });
  document.addEventListener("focusout", () => host && requestAnimationFrame(() => host && !wanted(host) && hide()));
  // something scrolled: the tip goes only if its host moved with it
  document.addEventListener(
    "scroll",
    () => {
      const r = host?.getBoundingClientRect();
      if (r && (Math.abs(r.left - at.x) > 1 || Math.abs(r.top - at.y) > 1)) hide();
    },
    true,
  );
  document.addEventListener("pointerdown", () => host && requestAnimationFrame(() => host && !wanted(host) && hide()), true);
  window.addEventListener("blur", hide);
  window.addEventListener("resize", hide);
}
