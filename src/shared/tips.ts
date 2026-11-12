// 落签 (the tooltip hanging on a string, Design System Tooltip: an element with `pm-tip-host` and a `.pm-tip` inside)
// is shown in a layer above everything, so no page, card or scrolling box can cut it off: while a host is pointed at
// (or has keyboard focus) a copy of its tip hangs from a fixed-positioned stand-in at the host's place, in the page —
// or in the open dialog, which sits above the page — and the tip inside the host stays hidden (index.css
// [data-tip-up]; hidden, not removed, so nothing around it moves). The copy swings with the Design System's own keyframes; near the window's edge the tag slides in
// along its string. A host given `is-hover` by code (柚子瓣's typed match) still shows its own tip in place (`hangAt`).
// The string hangs a little under the drawing itself, not under the host's box (whose padding left a gap): `inkBottom`
// finds where the icon's or sticker's lines end straight under the host's middle (or the host's own box, when pointing
// at it paints a background behind the icon). A host that moves when pointed at (a page in the stack slides out and
// its sticker tilts) is followed until it settles, so the string stays under it.
let host: HTMLElement | null = null;
let float: HTMLElement | null = null;
let watch = 0;
let raf = 0;
let at = { x: 0, y: 0, w: 0, h: 0 };
const EDGE = 8;
/** The little gap between the drawing and the top of the string (the tag hangs near the icon, not stuck to it). */
export const GAP = 3;
/** How long the stand-in keeps measuring after it was hung or last moved (the hover transitions take up to 0.5s). */
const SETTLE = 600;
const SHAPES = "path, circle, ellipse, rect, line, polyline, polygon";
/** A color that paints: not none, not transparent, alpha above 0 (rgba(…, 0) and the slash forms like oklab(… / 0)). */
const painted = (c: string) => !!c && c !== "none" && c !== "transparent" && !/^rgba\(.*,\s*0(?:\.0*)?\s*\)$|\/\s*0(?:\.0*)?\s*\)$/.test(c);

/** The lowest point (screen y) where the host's own drawing is under x (its middle); null: nothing drawn there. Each
 *  SVG shape is tested where it is painted (fill or stroke), so a tilted sticker or a thin icon is met where it really is;
 *  a shape whose position can't be trusted (getScreenCTM disagreeing with the layout) counts by its box. A host whose
 *  own box is painted (an icon button's hover background) counts as drawn down to its bottom. */
export function inkBottom(h: Element, x: number): number | null {
  let best: number | null = null;
  const hs = getComputedStyle(h);
  if (painted(hs.backgroundColor) || hs.backgroundImage !== "none") {
    const r = h.getBoundingClientRect();
    if (r.left <= x && x <= r.right && r.height) best = r.bottom;
  }
  for (const el of h.querySelectorAll<SVGGeometryElement>(SHAPES)) {
    if (!(el instanceof SVGGeometryElement) || el.closest(".pm-tip, defs, symbol, clipPath, mask, pattern, marker, filter")) continue;
    const box = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    if (!box.width && !box.height) continue;
    if (cs.visibility === "hidden" || cs.display === "none" || cs.opacity === "0") continue;
    const sw = parseFloat(cs.strokeWidth) || 0;
    const fill = painted(cs.fill) && parseFloat(cs.fillOpacity) !== 0;
    const stroke = painted(cs.stroke) && sw > 0 && parseFloat(cs.strokeOpacity) !== 0;
    if ((!fill && !stroke) || box.left - sw > x || box.right + sw < x || (best !== null && box.bottom + sw <= best)) continue;
    const svg = el.ownerSVGElement;
    const m = el.getScreenCTM();
    let trusted = !!(svg && m);
    if (svg && m) {
      // the shape's own box through the matrix must land where the layout put it
      const g = el.getBBox();
      const ps = [[g.x, g.y], [g.x + g.width, g.y], [g.x, g.y + g.height], [g.x + g.width, g.y + g.height]].map(([px, py]) => new DOMPoint(px, py).matrixTransform(m));
      const ys = ps.map((p) => p.y), xs = ps.map((p) => p.x);
      trusted = Math.abs(Math.min(...xs) - box.left) < 2.5 + sw && Math.abs(Math.max(...ys) - box.bottom) < 2.5 + sw;
    }
    if (!trusted || !svg || !m) {
      best = Math.max(best ?? -Infinity, box.bottom);
      continue;
    }
    const inv = m.inverse();
    const p = svg.createSVGPoint();
    for (let y = box.bottom + sw; y >= box.top - sw && (best === null || y > best); y -= 0.5) {
      p.x = x;
      p.y = y;
      const q = p.matrixTransform(inv);
      if ((fill && el.isPointInFill(q)) || (stroke && el.isPointInStroke(q))) {
        best = y;
        break;
      }
    }
  }
  return best;
}

/** Hang a host's own tip (shown in place, `is-hover`) from its drawing: its `top` in the host's own pixels. */
export function hangAt(h: HTMLElement) {
  const tip = h.querySelector<HTMLElement>(":scope > .pm-tip");
  if (!tip) return;
  const r = h.getBoundingClientRect();
  const ink = inkBottom(h, r.left + r.width / 2);
  const k = h.offsetHeight ? r.height / h.offsetHeight : 1;
  tip.style.top = ink === null ? "" : `${(ink - r.top + GAP) / (k || 1)}px`;
}

function hide() {
  host?.removeAttribute("data-tip-up");
  float?.remove();
  host = float = null;
  window.clearInterval(watch);
  cancelAnimationFrame(raf);
  raf = 0;
}

const wanted = (h: HTMLElement) => h.isConnected && h.matches(":hover, :focus-visible, :has(> :focus-visible)");

/** Put the stand-in where the host is now, the string under its drawing and the tag inside the window. */
function place(h: HTMLElement, stand: HTMLElement, copy: HTMLElement, css: number) {
  const r = h.getBoundingClientRect();
  stand.style.left = `${r.left}px`;
  stand.style.top = `${r.top}px`;
  stand.style.width = `${r.width}px`;
  stand.style.height = `${r.height}px`;
  const mid = r.left + r.width / 2;
  const ink = inkBottom(h, mid);
  copy.style.top = `${ink !== null ? ink - r.top + GAP : Number.isFinite(css) ? css : r.height + 4}px`;
  at = { x: r.left, y: r.top, w: r.width, h: r.height };
  // the tag stays inside the window (its string keeps hanging from the middle of the host)
  const tag = copy.querySelector<HTMLElement>(".pm-tip-tag");
  if (tag) {
    const w = tag.offsetWidth;
    const dx = Math.min(0, window.innerWidth - EDGE - (mid + w / 2)) + Math.max(0, EDGE - (mid - w / 2));
    tag.style.translate = dx ? `${dx}px 0` : "";
  }
}

const moved = (r: DOMRect) => Math.abs(r.left - at.x) > 0.25 || Math.abs(r.top - at.y) > 0.25 || Math.abs(r.width - at.w) > 0.25 || Math.abs(r.height - at.h) > 0.25;

/** Measure every frame until nothing has moved for SETTLE ms (the host's own hover transition, a sticker tilting). */
function follow(h: HTMLElement, stand: HTMLElement, copy: HTMLElement, css: number) {
  cancelAnimationFrame(raf);
  let until = performance.now() + SETTLE;
  const tick = (now: number) => {
    if (host !== h) return;
    if (moved(h.getBoundingClientRect())) until = now + SETTLE;
    place(h, stand, copy, css);
    raf = now < until ? requestAnimationFrame(tick) : 0;
  };
  raf = requestAnimationFrame(tick);
}

function show(h: HTMLElement) {
  if (h === host) return;
  hide();
  const tip = h.querySelector<HTMLElement>(":scope > .pm-tip");
  if (!tip) return;
  const css = parseFloat(getComputedStyle(tip).top);
  const stand = document.createElement("span");
  stand.className = "pm-tip-host is-hover tip-float";
  stand.setAttribute("aria-hidden", "true");
  stand.style.position = "fixed";
  const copy = tip.cloneNode(true) as HTMLElement;
  copy.removeAttribute("role");
  stand.appendChild(copy);
  (h.closest("dialog[open]") ?? document.body).appendChild(stand);
  h.setAttribute("data-tip-up", "");
  host = h;
  float = stand;
  place(h, stand, copy, css);
  follow(h, stand, copy, css);
  watch = window.setInterval(() => {
    if (host && !wanted(host)) hide();
    else if (host && !raf && moved(host.getBoundingClientRect())) follow(host, stand, copy, css);
  }, 200);
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
