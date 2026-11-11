// 落签 (the tooltip hanging on a string, Design System Tooltip: an element with `pm-tip-host` and a `.pm-tip` inside)
// is shown in a layer above everything, so no page, card or scrolling box can cut it off: while a host is pointed at
// (or has keyboard focus) a copy of its tip hangs from a fixed-positioned stand-in at the host's place, in the page —
// or in the open dialog, which sits above the page — and the tip inside the host stays hidden (index.css
// [data-tip-up]; hidden, not removed, so nothing around it moves). The copy swings with the Design System's own keyframes; near the window's edge the tag slides in
// along its string. A host given `is-hover` by code (柚子瓣's typed match) still shows its own tip in place (`hangAt`).
// The string starts on the drawing itself, not under the host's box (whose padding left a gap): `inkBottom` finds where
// the icon's or sticker's lines end straight under the host's middle.
let host: HTMLElement | null = null;
let float: HTMLElement | null = null;
let watch = 0;
let at = { x: 0, y: 0 };
const EDGE = 8;
/** How far the string reaches up into the line it hangs from, so no hairline shows between them (a `pm-press` host also
 *  rises 1px when pointed at). */
const TUCK = 1.5;
const SHAPES = "path, circle, ellipse, rect, line, polyline, polygon";
const painted = (c: string) => c !== "none" && c !== "transparent" && !/^rgba\(.*,\s*0\)$/.test(c);

/** The lowest point (screen y) where the host's own drawing is under x (its middle); null: nothing drawn there. Each
 *  SVG shape is tested where it is painted (fill or stroke), so a tilted sticker or a thin icon is met where it really is;
 *  a shape whose position can't be trusted (getScreenCTM disagreeing with the layout) counts by its box. */
export function inkBottom(h: Element, x: number): number | null {
  let best: number | null = null;
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
  tip.style.top = ink === null ? "" : `${(ink - r.top - TUCK) / (k || 1)}px`;
}

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
  const ink = inkBottom(h, r.left + r.width / 2);
  copy.style.top = `${ink !== null ? ink - r.top - TUCK : Number.isFinite(top) ? top : r.height + 4}px`;
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
