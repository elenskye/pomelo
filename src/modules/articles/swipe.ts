// Two fingers sideways on 见字如面 (after Bear). Anywhere on the page: left folds the tags, then the list; right unfolds.
// Over a row of the list: left draws the row aside and shows its delete behind it — a round button, stretching into a pill
// as the fingers go on; let go and it rests open (click the button to delete) or, drawn only a little, closes; right
// closes an open row. Letting go never deletes (站主 11-15: too hard to judge how far). The mouse does the same: press on
// a row and drag it sideways. Wheel events carry no phase, so a swipe is told from the glide macOS sends after the fingers
// lift (Glide): the glide never counts, and a new swipe during it counts at once — no click needed between swipes; one
// swipe folds one column however late or lumped its events come (scripts/swipe-sim.mjs plays the cases). The listener is
// on the window (WebKit hands a gesture to the handler that took its first event), so the page needn't have been clicked.
import { useEffect, useRef } from "react";

/** Splits wheel deltas into swipes: `id` changes with each new one, `gliding` once the fingers are off.
 * Wheel events carry no phase, and they don't always come as they were made: while the app is busy (a save and the
 * lists it refreshes, a column folding and the article laying out again) they wait, then several come as one — their
 * deltas added — or late in a burst. Read one at a time that looked like a pause and a new start, or like the fingers
 * speeding up again, and one swipe folded two columns. So a new swipe is told only by what a hand does:
 * - it turns back (a glide never does);
 * - it starts after a real pause: by the events' own times and with the page awake meanwhile (`awake`: how long it
 *   ran and got no wheel event — a pause it slept through is not one), and not with a big delta the glide could have
 *   covered meanwhile at its last speed;
 * - during a glide: fingers coming down stop it, so there is a short real pause (the page awake all through it), then
 *   a small delta, then the speed (px per ms, which adding events up doesn't change) rises twice in a row.
 * A swipe that did its work rests (`rest`): nothing new for a moment, however the events come. */
export class Glide {
  private t = -1e9;
  /** The last events' speed, smoothed; the last one's alone. */
  private v = 0;
  private last = 0;
  private peak = 0;
  private falling = 0;
  /** The glide stopped for a moment and went on small: fingers may be back. */
  private paused = false;
  private rising = 0;
  private dir = 0;
  private hold = -1e9;
  private spent = false;
  id = 0;
  acc = 0;
  gliding = false;

  /** This swipe did what it was for (a column folded): no new one for a moment. */
  rest(now: number) {
    this.hold = now + 300;
    this.spent = true;
    this.paused = false;
    this.rising = 0;
  }

  /** One event's sideways delta at its own time; `awake`: ms the page ran with no wheel event before it, `slept`: and
   * didn't run for a moment in between. True when it starts a new swipe. */
  feed(dx: number, now: number, awake = Infinity, slept = false): boolean {
    const a = Math.abs(dx);
    const dir = Math.sign(dx);
    const dt = now - this.t;
    const quiet = dt > 160 && awake >= 100;
    const speed = a / Math.max(dt, 4);
    const held = now < this.hold;
    const after = this.gliding || this.spent;
    let fresh = false;
    if (dir !== 0 && this.dir !== 0 && dir !== this.dir && a > 1.5) fresh = true;
    else if (quiet) {
      // (a glide held back may also be fingers back with a big first delta: the rise after it tells)
      const glide = after && a > 8 && dt < 1200 && a <= this.v * dt * 1.2 + 8;
      if (glide || held) {
        this.paused = true;
        this.rising = 0;
      } else fresh = true;
    } else if (after) {
      if (dt >= 28 && awake >= 22 && !slept && (a <= 4 || speed < this.v * 0.3)) {
        this.paused = true;
        this.rising = 0;
      } else if (this.paused) {
        if (this.rising < 2) this.rising = speed > 0.25 && speed > this.last * 1.05 ? this.rising + 1 : 0;
        if (this.rising >= 2 && !held) fresh = true;
      }
    }
    this.t = now;
    this.last = speed;
    const before = this.v;
    this.v = dt > 160 ? speed : before * 0.5 + speed * 0.5;
    if (fresh) {
      this.id++;
      this.acc = 0;
      this.peak = 0;
      this.falling = 0;
      this.rising = 0;
      this.paused = false;
      this.gliding = false;
      this.spent = false;
      if (dir) this.dir = dir;
    }
    this.acc += dx;
    if (this.v > this.peak) this.peak = this.v;
    this.falling = this.v <= before * 1.02 ? this.falling + 1 : 0;
    if (!this.gliding && this.falling >= 3 && this.v < this.peak * 0.65) this.gliding = true;
    return fresh;
  }
}

/** How far a row opens to rest. */
const OPEN = 76;
const EASE = "cubic-bezier(0.2, 0.8, 0.2, 1)";

type Row = { box: HTMLElement; row: HTMLElement; del: HTMLElement; pill: HTMLElement; id: number; off: number };

function rowOf(el: Element | null): Row | null {
  const box = el?.closest<HTMLElement>(".ar-swipe[data-id]");
  const row = box?.querySelector<HTMLElement>(":scope > .ar-row");
  const del = box?.querySelector<HTMLElement>(":scope > .ar-swipe-del");
  const pill = del?.querySelector<HTMLElement>(".ar-swipe-pill");
  if (!box || !row || !del || !pill) return null;
  return { box, row, del, pill, id: Number(box.dataset.id), off: Number(box.dataset.off ?? 0) };
}

/** Draws a row drawn aside by `off` px: the row slides, the button fills what it leaves (round, then a pill). */
function paint(r: Row, off: number, ms = 0) {
  r.off = off;
  r.box.dataset.off = String(off);
  const t = ms ? `transform ${ms}ms ${EASE}` : "";
  r.row.style.transition = t;
  r.del.style.transition = ms ? `width ${ms}ms ${EASE}` : "";
  r.pill.style.transition = ms ? `width ${ms}ms ${EASE}, transform ${ms}ms ${EASE}, opacity ${ms}ms` : "";
  r.row.style.transform = off ? `translateX(${-off}px)` : "";
  r.del.style.width = `${off}px`;
  const grow = Math.min(1, off / 56);
  r.pill.style.width = `${Math.max(38, off - 22)}px`;
  r.pill.style.opacity = String(Math.min(1, off / 30));
  r.pill.style.transform = `scale(${0.55 + 0.45 * grow})`;
  r.box.classList.toggle("open", off > 0);
}

export function useSwipes(opts: {
  /** The page's columns (the page is up when it's on screen). */
  cols: React.RefObject<HTMLElement | null>;
  /** +1 fold one more, -1 one less. */
  onFold: (step: 1 | -1) => void;
  /** The button clicked: what to do once the row has gone (null: it stays, put it back). */
  onDelete: (id: number) => Promise<(() => Promise<unknown>) | null>;
}) {
  const o = useRef(opts);
  o.current = opts;
  useEffect(() => {
    const glide = new Glide();
    /** Who the current swipe belongs to: a row (from its offset at the start) or the folding. */
    let owner: { id: number; row: Row | null; from: number; done: boolean } = { id: -1, row: null, from: 0, done: true };
    let open: Row | null = null;
    let idle = 0;
    let frame = 0;
    // Glide's `awake` and `slept`: a beat every 8 ms while swipes are about (until 3 s after the last event) adds up how
    // long the page ran since the last sideways event, and notes a beat that came late (the page stood still)
    let beat = 0;
    let beatAt = 0;
    let beatOff = 0;
    let awake = 0;
    let slept = false;
    const onBeat = () => {
      const n = performance.now();
      if (n - beatAt > 30) slept = true;
      awake += Math.min(n - beatAt, 20);
      beatAt = n;
    };
    const heard = (): [number, boolean] => {
      const n = performance.now();
      const out: [number, boolean] = beat ? [awake + Math.min(n - beatAt, 20), slept || n - beatAt > 30] : [Infinity, false];
      awake = 0;
      slept = false;
      beatAt = n;
      if (!beat) beat = window.setInterval(onBeat, 8);
      window.clearTimeout(beatOff);
      beatOff = window.setTimeout(() => {
        window.clearInterval(beat);
        beat = 0;
      }, 3000);
      return out;
    };
    const close = (r: Row | null, ms = 220) => r && r.box.isConnected && paint(r, 0, ms);
    /** Slides the row out, folds its place away, then `then` (failing: the row comes back). */
    const remove = (r: Row, then: () => Promise<unknown>) => {
      paint(r, r.box.clientWidth, 200);
      window.setTimeout(() => {
        r.box.style.transition = `height 180ms ${EASE}, opacity 180ms`;
        r.box.style.height = `${r.box.offsetHeight}px`;
        void r.box.offsetHeight;
        r.box.style.height = "0px";
        r.box.style.opacity = "0";
      }, 190);
      window.setTimeout(() => {
        then().catch(() => {
          r.box.style.transition = "";
          r.box.style.height = "";
          r.box.style.opacity = "";
          close(r);
        });
      }, 380);
    };
    const destroy = async (r: Row) => {
      if (open?.id === r.id) open = null;
      const then = await o.current.onDelete(r.id).catch(() => null);
      if (then && r.box.isConnected) remove(r, then);
      else close(r);
    };
    const settle = (r: Row) => {
      if (r.off >= 36) {
        paint(r, OPEN, 240);
        open = r;
      } else {
        close(r);
        if (open?.id === r.id) open = null;
      }
    };
    const release = () => {
      window.clearTimeout(idle);
      cancelAnimationFrame(frame);
      if (owner.done) return;
      owner.done = true;
      if (owner.row) settle(owner.row);
    };

    const onWheel = (e: WheelEvent) => {
      const cols = o.current.cols.current;
      if (!cols || !cols.isConnected || !cols.getClientRects().length) return;
      // only on this page's paper while it's the front one, never over a dialog or a menu
      const page = cols.closest(".pm-page");
      const t = e.target instanceof Element ? e.target : null;
      if (!t || (page ? !page.classList.contains("front") || !page.contains(t) : !cols.contains(t))) return;
      if (t.closest('[role="dialog"], [role="menu"], [role="listbox"], .pm-menu')) return;
      const sideways = Math.abs(e.deltaX) > Math.abs(e.deltaY) * 1.2;
      if (!sideways) {
        // scrolling the list puts an open row back
        if (open && Math.abs(e.deltaY) > 2) (close(open), (open = null));
        return;
      }
      // something that scrolls sideways itself (a wide table, a formula, a diagram) keeps the swipe while it can go on
      for (let x: Element | null = t; x && x !== document.body; x = x.parentElement) {
        if (!(x instanceof HTMLElement) || x.scrollWidth <= x.clientWidth + 2 || !/(auto|scroll)/.test(getComputedStyle(x).overflowX)) continue;
        if (e.deltaX > 0 ? x.scrollLeft + x.clientWidth < x.scrollWidth - 1 : x.scrollLeft > 0) return;
      }
      e.preventDefault();
      const fresh = glide.feed(e.deltaX, e.timeStamp, ...heard());
      if (fresh) {
        release();
        const r = rowOf(t);
        const rowOpen = r && open?.id === r.id ? open : null;
        if (r && (e.deltaX > 0 || rowOpen)) {
          if (open && open.id !== r.id) (close(open), (open = null));
          owner = { id: glide.id, row: r, from: rowOpen ? rowOpen.off : r.off, done: false };
        } else {
          if (open) (close(open), (open = null));
          owner = { id: glide.id, row: null, from: 0, done: false };
        }
      }
      if (owner.done || owner.id !== glide.id) return;
      if (glide.gliding) return release();
      window.clearTimeout(idle);
      idle = window.setTimeout(release, 140);
      const r = owner.row;
      if (r) {
        // where it is now (letting go reads it), drawn on the next frame
        r.off = Math.max(0, Math.min(r.box.clientWidth, owner.from + glide.acc));
        r.box.dataset.off = String(r.off);
        cancelAnimationFrame(frame);
        frame = requestAnimationFrame(() => paint(r, r.off));
        return;
      }
      if (Math.abs(glide.acc) >= 50) {
        owner.done = true;
        glide.rest(e.timeStamp);
        o.current.onFold(glide.acc > 0 ? 1 : -1);
      }
    };
    // a click anywhere else puts an open row back; its button deletes
    const onDown = (e: PointerEvent) => {
      if (!open) return;
      const t = e.target instanceof Element ? e.target : null;
      // on the open row itself: a drag moves it on, a click puts it back (onUp)
      if (rowOf(t)?.id === open.id && (t?.closest(".ar-swipe-del") || (e.pointerType === "mouse" && e.button === 0))) return;
      close(open);
      open = null;
    };
    const onDel = (e: MouseEvent) => {
      const t = e.target instanceof Element ? e.target.closest(".ar-swipe-del") : null;
      const r = t && rowOf(t);
      if (!r) return;
      e.preventDefault();
      e.stopPropagation();
      void destroy(r);
    };
    // the mouse: press on a row and drag it sideways (past a few pixels, and more sideways than up and down)
    let drag: { r: Row; x: number; y: number; from: number; on: boolean; pid: number } | null = null;
    let swallow = false;
    const onPress = (e: PointerEvent) => {
      if (e.button !== 0 || e.pointerType !== "mouse" || e.metaKey || e.shiftKey || e.ctrlKey || e.altKey) return;
      const t = e.target instanceof Element ? e.target : null;
      if (!t?.closest(".ar-swipe > .ar-row")) return;
      const r = rowOf(t);
      if (!r) return;
      drag = { r, x: e.clientX, y: e.clientY, from: open?.id === r.id ? open.off : 0, on: false, pid: e.pointerId };
    };
    const onMove = (e: PointerEvent) => {
      if (!drag || e.pointerId !== drag.pid) return;
      const dx = e.clientX - drag.x;
      if (!drag.on) {
        if (Math.abs(dx) < 6 || Math.abs(dx) < Math.abs(e.clientY - drag.y) * 1.2) {
          if (Math.abs(e.clientY - drag.y) > 8) drag = null;
          return;
        }
        drag.on = true;
        if (open && open.id !== drag.r.id) (close(open), (open = null));
        window.getSelection()?.removeAllRanges();
      }
      e.preventDefault();
      const r = drag.r;
      r.off = Math.max(0, Math.min(r.box.clientWidth, drag.from - dx));
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => paint(r, r.off));
    };
    const onUp = (e: PointerEvent) => {
      if (!drag || e.pointerId !== drag.pid) return;
      const d = drag;
      drag = null;
      if (!d.on) {
        if (open?.id !== d.r.id) return;
        // a click on the open row puts it back, and doesn't open it
        close(open);
        open = null;
        swallow = true;
        window.setTimeout(() => (swallow = false), 0);
        return;
      }
      swallow = true; // the click this release makes doesn't open the row
      window.setTimeout(() => (swallow = false), 0);
      cancelAnimationFrame(frame);
      settle(d.r);
    };
    const onClickSwallow = (e: MouseEvent) => {
      if (!swallow) return;
      swallow = false;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("wheel", onWheel, { passive: false });
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("pointerdown", onPress);
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
    window.addEventListener("click", onClickSwallow, true);
    window.addEventListener("click", onDel, true);
    return () => {
      window.removeEventListener("wheel", onWheel);
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("pointerdown", onPress);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      window.removeEventListener("click", onClickSwallow, true);
      window.removeEventListener("click", onDel, true);
      window.clearTimeout(idle);
      window.clearTimeout(beatOff);
      window.clearInterval(beat);
      cancelAnimationFrame(frame);
    };
  }, []);
}
