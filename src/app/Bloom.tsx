// 柚子瓣 (requirements「界面外框」; visual-atelier's frame prototype, board Bloom, approved 2026-11-08): the pomelo
// opens into a ring of wedges, one per category in its color, inside a hand-drawn rind with pores, gaps between the
// wedges. Only stickers in the wedges, no names (the name hangs under a sticker on a string when you point at it);
// each wedge's stickers zigzag between an outer and an inner ring, clear of its edges. The pomelo, today's date and
// the box sit right in the middle. Drag to spin it (it coasts on); type to find (the rest fade, the found ones show
// their name; ↩ opens the first, or asks 柚子 when nothing matches); type 「柚子」 and it spins round once.
// Right-click a sticker for 置顶 / 隐藏 / 图标和颜色 / 移到; 「＋ 分类」 and 已隐藏 sit in the corners.
// Motion: ds.css (wheel-open, wheel-spin, tooltip); geometry and drawing as the prototype (gen.mjs bloom()).
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { flash, reducedMotion } from "@/shared/hand";
import { HandIcon, Tip } from "@/shared/handParts";
import { hangAt } from "@/shared/tips";
import { CatName, liveKey, liveText, ModuleRow, useModuleMenu } from "./DockPage";
import { catTint, useDock } from "./dock";
import { Menu } from "./Menu";
import { ModuleIcon } from "./ModuleIcon";
import { PomeloFace } from "./Pomelo";
import { useTabs } from "./tabs";

// the prototype's geometry: Phase 2's 600 grid scaled to 680
const K = 680 / 600;
const W = { C: 340, R1: 120 * K, R2: 244 * K, RR1: 252 * K, RR2: 272 * K, RL: 304 * K, PITH: 112 * K, RO: 237, RI: 177, ICON: 54 };
const pt = (r: number, deg: number): [number, number] => [W.C + r * Math.cos((deg * Math.PI) / 180), W.C + r * Math.sin((deg * Math.PI) / 180)];
const f = (n: number) => +n.toFixed(2);

/** A wedge's stickers: outer, inner, outer… evenly along it, each end clear of the edge by the sticker's radius + 7. */
function zigzag(n: number, a0: number, a1: number): [number, number][] {
  if (n === 1) return [[(W.RO + W.RI) / 2, (a0 + a1) / 2]];
  const pad = (r: number) => ((W.ICON / 2 + 7) / r) * 57.2958;
  const r = (k: number) => (k % 2 ? W.RI : W.RO);
  const start = a0 + pad(r(0)), end = a1 - pad(r(n - 1));
  return Array.from({ length: n }, (_, k) => [r(k), start + ((end - start) * k) / (n - 1)]);
}

type Hand = { P: (d: string) => unknown; handFill: (c: unknown, o: object) => string; handStroke: (c: unknown, o: object) => string };
const hand = () => (window as unknown as { Pomelo?: { hand?: Hand } }).Pomelo?.hand ?? null;
const poly = (pts: [number, number][], close = true) => "M" + pts.map(([x, y]) => `${f(x)} ${f(y)}`).join(" L") + (close ? " Z" : "");
const circlePts = (r: number) => Array.from({ length: 73 }, (_, i) => pt(r, i * 5));

const today = () => {
  const d = new Date();
  return `${d.getMonth() + 1} 月 ${d.getDate()} 日 · 周${"日一二三四五六"[d.getDay()]}`;
};

export function Bloom(props: { anchor: HTMLElement | null; onClose: () => void; onAsk: ((text: string) => void) | null }) {
  const dock = useDock();
  const tabs = useTabs();
  const live = useQuery({ queryKey: liveKey, queryFn: liveText, staleTime: 0 }).data ?? {};
  const menu = useModuleMenu(dock);
  const [q, setQ] = useState("");
  const [closing, setClosing] = useState(false);
  const [wow, setWow] = useState(true);
  // the opening's pop is over (still): each sticker is built afresh. WebKit keeps a sticker's SVG shadow filter
  // drawn at the size it first had — tiny, mid-pop — and stretches it: a blurred, half-white square over the filter's
  // area (念念不忘's slim leaf leaves most of it bare, so it shows). Turning the wheel cleared it only because
  // --pm-a is inherited, so every sticker's style changed and WebKit drew the shadow again.
  const [still, setStill] = useState(false);
  const [showHidden, setShowHidden] = useState(false);
  const [editCat, setEditCat] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const whl = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const ang = useRef(0);
  const moved = useRef(false);

  const close = () => {
    if (closing) return;
    if (reducedMotion()) return props.onClose();
    setClosing(true);
    setTimeout(props.onClose, 300);
  };
  const go = (id: string) => {
    tabs.open(`/${id}`);
    props.onClose();
  };

  // scaled to fit; grows out of the pomelo (its centre, in the wheel's own units)
  useLayoutEffect(() => {
    const fit = () => {
      const b = box.current, w = whl.current;
      if (!b || !w) return;
      const k = Math.min(1, (b.clientHeight - 60) / 700, (b.clientWidth - 40) / 700);
      b.style.setProperty("--k", String(k));
      const br = b.getBoundingClientRect();
      const p = props.anchor?.getBoundingClientRect();
      if (p) {
        w.style.setProperty("--pm-from-x", `${(p.left + p.width / 2 - (br.left + br.width / 2)) / k}px`);
        w.style.setProperty("--pm-from-y", `${(p.top + p.height / 2 - (br.top + br.height / 2)) / k}px`);
      }
    };
    fit();
    window.addEventListener("resize", fit);
    return () => window.removeEventListener("resize", fit);
  }, [props.anchor]);
  useEffect(() => {
    input.current?.focus({ preventScroll: true });
    const t = setTimeout(() => setWow(false), 900);
    return () => clearTimeout(t);
  }, []);
  // Esc closes (a menu, a drag or a name being edited take theirs first)
  const closeRef = useRef(close);
  closeRef.current = close;
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || document.querySelector("dialog[open]")) return;
      e.preventDefault();
      closeRef.current();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);

  const cats = dock.layout.cats.map((c) => ({ c, mods: c.mods.filter((m) => !dock.isHidden(m)) }));
  const slots = cats.reduce((n, x) => n + Math.max(x.mods.length, 1), 0) || 1;
  const step = 360 / slots;
  const mods = cats.reduce((n, x) => n + x.mods.length, 0);
  useEffect(() => {
    const t = setTimeout(() => setStill(true), 300 + mods * 11 + 350 + 100);
    return () => clearTimeout(t);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const layout = useMemo(() => {
    let a = -90 - (Math.max(cats[0]?.mods.length ?? 1, 1) * step) / 2;
    return cats.map(({ c, mods }) => {
      const a0 = a, a1 = a + Math.max(mods.length, 1) * step;
      a = a1;
      return { c, mods, a0, a1, at: mods.length ? zigzag(mods.length, a0, a1) : [] };
    });
  }, [JSON.stringify(cats), step]); // eslint-disable-line react-hooks/exhaustive-deps

  // the drawing: rind (two ink lines, a ring of pores), the wedges (a gap between each), the pith
  const art = useMemo(() => {
    const h = hand();
    const sector = (a0: number, a1: number) => {
      const g1 = (4 / W.R2) * 57.3, g0 = (4 / W.R1) * 57.3;
      const pts: [number, number][] = [];
      for (let t = a0 + g1; t <= a1 - g1 + 0.01; t += 3) pts.push(pt(W.R2, t));
      pts.push(pt(W.R2, a1 - g1));
      for (let t = a1 - g0; t >= a0 + g0 - 0.01; t -= 3) pts.push(pt(W.R1, t));
      pts.push(pt(W.R1, a0 + g0));
      return poly(pts);
    };
    const stroke = (d: string, o: object) => (h ? h.handStroke(h.P(d), { sp: 3, keep: 3, prec: 0, ...o }) : d);
    const fill = (d: string, o: object) => (h ? h.handFill(h.P(d), { sp: 3, keep: 3, prec: 0, ...o }) : d);
    return {
      wedges: layout.map((w, ci) => ({ id: w.c.id, tint: catTint(w.c), fill: fill(sector(w.a0, w.a1), { amp: 0.8, seed: 600 + ci }), line: stroke(sector(w.a0, w.a1), { amp: 1.1, seed: 610 + ci, over: 0.02 }) })),
      rindFill: fill(poly(circlePts(W.RR2)), { amp: 1, seed: 620 }) + fill(poly(circlePts(W.RR1)), { amp: 0.8, seed: 621 }),
      rindOut: stroke(poly(circlePts(W.RR2)), { amp: 1.3, seed: 622, over: 0.05 }),
      rindIn: stroke(poly(circlePts(W.RR1)), { amp: 1, seed: 623, over: 0.05 }),
      pores: Array.from({ length: 32 }, (_, i) => pt(262 * K, i * 11.25 + 4)),
      pithFill: fill(poly(circlePts(W.PITH)), { amp: 0.8, seed: 630, sp: 2.5, prec: 2 }),
      pithLine: stroke(poly(circlePts(W.PITH)), { amp: 1, seed: 631, over: 0.05, sp: 2.5, prec: 2 }),
    };
  }, [layout]);

  const term = q.trim().toLowerCase();
  const matches = term ? layout.flatMap((w) => w.mods).filter((m) => dock.defs.get(m)!.label.toLowerCase().includes(term)) : [];
  const first = matches[0] ?? null;
  const hidden = dock.hidden.filter((m) => dock.defs.has(m));
  // the found ones show their name in place: its string starts on the sticker itself
  const found = matches.join(" ");
  useLayoutEffect(() => {
    for (const h of whl.current?.querySelectorAll<HTMLElement>(".slotlift.is-hover") ?? []) hangAt(h);
  }, [found]);
  // 小彩蛋: type 「柚子」 and it spins round once
  useEffect(() => {
    if (q.trim() === "柚子" && !reducedMotion()) flash(whl.current, "is-spin", 920);
  }, [q]);

  // drag to spin, it coasts on (×0.95 a frame); no drag = a click on the sticker under the pointer
  const setAng = (a: number) => {
    ang.current = a;
    whl.current?.style.setProperty("--pm-a", `${a.toFixed(2)}deg`);
  };
  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("input,.nodrag")) return;
    const el = e.currentTarget;
    const r = el.getBoundingClientRect(), cx = r.left + r.width / 2, cy = r.top + r.height / 2;
    const angOf = (x: number, y: number) => (Math.atan2(y - cy, x - cx) * 180) / Math.PI;
    let last = angOf(e.clientX, e.clientY), w = 0, lt = performance.now();
    const x0 = e.clientX, y0 = e.clientY;
    moved.current = false;
    el.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      if (Math.abs(ev.clientX - x0) + Math.abs(ev.clientY - y0) > 4) moved.current = true;
      if (!moved.current) return;
      const a = angOf(ev.clientX, ev.clientY);
      let da = a - last;
      if (da > 180) da -= 360;
      if (da < -180) da += 360;
      const now = performance.now();
      w = 0.6 * w + 0.4 * (da / Math.max(1, now - lt)) * 16;
      lt = now;
      last = a;
      setAng(ang.current + da);
    };
    const up = (ev: PointerEvent) => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      if (!moved.current) {
        if (ev.type === "pointercancel") return;
        const slot = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>(".slot");
        if (slot?.dataset.go) go(slot.dataset.go);
        return;
      }
      if (performance.now() - lt > 90 || reducedMotion()) return;
      const coast = () => {
        w *= 0.95;
        setAng(ang.current + w);
        if (Math.abs(w) > 0.05) requestAnimationFrame(coast);
      };
      requestAnimationFrame(coast);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };

  let k = 0;
  const state = closing ? "is-closing" : "is-open";
  return (
    <div ref={box} className={`jp pm-scrim ${state}`} role="dialog" aria-label="柚子瓣" onClick={(e) => e.target === e.currentTarget && close()}>
      <div className="wheelbox">
        <div ref={whl} className={`whl pm-wheel ${state}`} onPointerDown={onPointerDown}>
          <svg className="pm-wheel-art" width="680" height="680" aria-hidden>
            <g className="spin pm-wheel-turn">
              <path d={art.rindFill} fill="var(--color-yuzu)" fillRule="evenodd" transform="translate(1.6 2)" />
              <path className="ol" pathLength={1} d={art.rindOut} fill="none" stroke="var(--color-pen)" strokeWidth="2.4" strokeLinecap="round" />
              <path className="ol" pathLength={1} d={art.rindIn} fill="none" stroke="var(--color-pen)" strokeWidth="1.8" strokeLinecap="round" />
              {art.pores.map(([x, y], i) => (
                <circle key={i} cx={f(x)} cy={f(y)} r="1.7" fill="color-mix(in srgb,var(--color-cat-orange) 55%,var(--color-yuzu))" />
              ))}
              {art.wedges.map((w) => (
                <g key={w.id}>
                  <path className="fl" d={w.fill} fill={`var(--color-cat-${w.tint}-tint)`} transform="translate(1.6 2)" />
                  <path className="ol" pathLength={1} d={w.line} fill="none" stroke="var(--color-pen)" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" />
                </g>
              ))}
            </g>
            <path d={art.pithFill} fill="var(--color-card)" />
            <path className="ol" pathLength={1} d={art.pithLine} fill="none" stroke="var(--color-pen)" strokeWidth="2" strokeLinecap="round" />
          </svg>
          <div className="spin pm-wheel-turn">
            {layout.flatMap((w) =>
              w.mods.map((m, j) => {
                const [x, y] = pt(...w.at[j]);
                const def = dock.defs.get(m)!;
                const hit = !!term && matches.includes(m);
                const i = k++;
                return (
                  <div
                    key={m}
                    className={`slot pm-wheel-slot ${term && !hit ? "dim" : ""} ${hit ? "hit" : ""}`}
                    data-go={m}
                    role="button"
                    aria-label={def.label}
                    title={live[m] || undefined}
                    style={{ left: f(x - 36), top: f(y - 36), "--pm-i": i } as React.CSSProperties}
                    onContextMenu={(e) => menu.at(e, m)}
                  >
                    <div className="slotin">
                      <div className={`slotlift pm-tip-host ${hit ? "is-hover" : ""}`}>
                        <span className="ic pm-lift" key={still ? "still" : "pop"}>
                          <span style={{ transform: `rotate(${[-6, 5, -3, 7, -5][i % 5]}deg)`, display: "inline-flex" }}>
                            <ModuleIcon def={def} size={W.ICON} />
                          </span>
                        </span>
                        <Tip>{def.label}</Tip>
                      </div>
                    </div>
                  </div>
                );
              }),
            )}
            {layout.map((w) => {
              const [lx, ly] = pt(W.RL, (w.a0 + w.a1) / 2);
              return (
                <div key={w.c.id} className="wl nodrag" style={{ left: f(lx - 40), top: f(ly - 13) }}>
                  <span style={{ color: `color-mix(in srgb,var(--color-cat-${catTint(w.c)}) 60%,var(--color-ink))` }}>
                    <CatName name={w.c.name} editing={editCat === w.c.id} onEdited={() => setEditCat(null)} onRename={(n) => dock.renameCat(w.c.id, n)} />
                  </span>
                </div>
              );
            })}
          </div>
          <div className="nodrag pithc">
            <PomeloFace face={wow ? "wow" : q.trim() === "柚子" ? "success" : "idle"} size={62} />
            <div className="soft">{today()}</div>
            <label className="pm-field wfield" data-hand="field" data-seed="640" data-r="12" data-fill="paper">
              <input
                ref={input}
                value={q}
                onChange={(e) => setQ(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    if (first) go(first);
                    else if (term && props.onAsk) {
                      props.onAsk(q.trim());
                      props.onClose();
                    }
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    e.stopPropagation();
                    close();
                  }
                }}
                placeholder="去哪，或问柚子"
                spellCheck={false}
                aria-label="去哪，或问柚子"
              />
            </label>
            {term && !first && props.onAsk && (
              <button
                type="button"
                className="wask pm-press"
                onClick={() => {
                  props.onAsk!(q.trim());
                  props.onClose();
                }}
              >
                <HandIcon name="sparkle" size={13} seed={641} />
                <span>问柚子「{q.trim()}」</span>
              </button>
            )}
          </div>
        </div>
      </div>
      <button type="button" className="corner left pm-press" onClick={() => setEditCat(dock.addCat())}>
        ＋ 分类
      </button>
      {hidden.length > 0 && (
        <div className="corner right">
          {showHidden && (
            <div className="hiddenlist">
              {hidden.map((m) => (
                <ModuleRow key={m} id={m} dock={dock} live={live[m]} menu={menu} from="hidden" onOpen={go} dim />
              ))}
            </div>
          )}
          <button type="button" className="pm-press" onClick={() => setShowHidden(!showHidden)}>
            已隐藏 {hidden.length}
          </button>
        </div>
      )}
      {menu.open && <Menu {...menu.open} onClose={menu.close} />}
      {menu.dialog}
    </div>
  );
}
