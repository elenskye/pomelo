// 柚子瓣 (requirements「界面外框」): the 模块坞 opened from the pomelo like a peeled pomelo — each wedge a
// category, its modules round the middle, names outside the rind (click to rename). The box in the centre
// filters; ↩ opens the first match, or asks 小拾 when nothing matches. Dragging, the 「⋯」 menu, 「＋ 分类」 and
// 已隐藏 work as in the 模块坞 (same layout). It grows out of the pomelo when it opens and stays put while
// it's being edited; Esc or a click outside closes it.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Bird, ChevronDown, ChevronUp, Ellipsis, EyeOff, Pin, Plus, X } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { CatName, liveKey, liveText, useModuleMenu, ModuleIcon, ModuleRow } from "./DockPage";
import { toneColor, useDock } from "./dock";
import { pressModule, radialDrop, useDragNow, type Over } from "./drag";
import { Menu } from "./Menu";
import { useTabs } from "./tabs";

const CX = 380,
  R1 = 128,
  R2 = 296,
  RM = 214,
  RL = 334,
  GAP = 3;
const pt = (r: number, deg: number): [number, number] => [CX + r * Math.cos((deg * Math.PI) / 180), CX + r * Math.sin((deg * Math.PI) / 180)];
function sector(r1: number, r2: number, a0: number, a1: number) {
  const lg = a1 - a0 > 180 ? 1 : 0,
    f = (n: number) => n.toFixed(1);
  const [x1, y1] = pt(r2, a0),
    [x2, y2] = pt(r2, a1),
    [x3, y3] = pt(r1, a1),
    [x4, y4] = pt(r1, a0);
  return `M${f(x1)} ${f(y1)}A${r2} ${r2} 0 ${lg} 1 ${f(x2)} ${f(y2)}L${f(x3)} ${f(y3)}A${r1} ${r1} 0 ${lg} 0 ${f(x4)} ${f(y4)}Z`;
}

const today = () => {
  const d = new Date();
  return `${d.getMonth() + 1} 月 ${d.getDate()} 日 · 周${"日一二三四五六"[d.getDay()]}`;
};

export function Bloom(props: { anchor: HTMLElement | null; onClose: () => void; onAsk: ((text: string) => void) | null }) {
  const dock = useDock();
  const tabs = useTabs();
  const live = useQuery({ queryKey: liveKey, queryFn: liveText, staleTime: 0 }).data ?? {};
  const drag = useDragNow();
  const menu = useModuleMenu(dock);
  const [q, setQ] = useState("");
  const [closing, setClosing] = useState(false);
  const [showHidden, setShowHidden] = useState(false);
  const [editCat, setEditCat] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const flower = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);

  const close = () => {
    if (closing) return;
    setClosing(true);
    setTimeout(props.onClose, 170);
  };
  const go = (id: string) => {
    tabs.open(`/${id}`);
    props.onClose();
  };

  // the flower's scale and where it grows from (the pomelo)
  useLayoutEffect(() => {
    const fit = () => {
      const b = box.current,
        f = flower.current;
      if (!b || !f) return;
      const k = Math.min(1, (b.clientHeight - 16) / 760, (b.clientWidth - 40) / 760);
      f.style.setProperty("--k", String(k));
      const br = b.getBoundingClientRect();
      const p = props.anchor?.getBoundingClientRect();
      if (p) {
        f.style.setProperty("--fx", `${p.left + p.width / 2 - (br.left + br.width / 2)}px`);
        f.style.setProperty("--fy", `${p.top + p.height / 2 - (br.top + br.height / 2)}px`);
      }
    };
    fit();
    window.addEventListener("resize", fit);
    return () => window.removeEventListener("resize", fit);
  }, [props.anchor]);
  useEffect(() => input.current?.focus(), []);
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
  const per = (360 - GAP * cats.length) / slots;
  let a = -90 - (per * Math.max(cats[0]?.mods.length ?? 1, 1)) / 2 - GAP / 2;
  const wedges = cats.map(({ c, mods }) => {
    const a0 = a + GAP / 2,
      a1 = a0 + per * Math.max(mods.length, 1);
    a = a1 + GAP / 2;
    return { c, mods, a0, a1 };
  });

  // the wedge under the pointer while dragging
  useEffect(() => {
    radialDrop.hit = (x, y): Over | null => {
      const f = flower.current?.getBoundingClientRect();
      if (!f) return null;
      const k = f.width / 760;
      const dx = (x - (f.left + f.width / 2)) / k,
        dy = (y - (f.top + f.height / 2)) / k,
        r = Math.hypot(dx, dy);
      if (r < R1 - 30 || r > RL + 40) return null;
      let deg = (Math.atan2(dy, dx) * 180) / Math.PI;
      const start = wedges[0]?.a0 ?? -90;
      while (deg < start) deg += 360;
      const w = wedges.find((x) => deg >= x.a0 - 2 && deg <= x.a1 + 2);
      if (!w) return null;
      return { zone: `cat:${w.c.id}`, before: w.mods[Math.floor((deg - w.a0) / per)] };
    };
    return () => {
      radialDrop.hit = null;
    };
  });

  const term = q.trim().toLowerCase();
  const matches = term ? wedges.flatMap((w) => w.mods).filter((m) => dock.defs.get(m)!.label.toLowerCase().includes(term)) : [];
  const first = matches[0] ?? null;
  const overCat = drag?.over?.zone.startsWith("cat:") ? drag.over.zone.slice(4) : null;
  const hidden = dock.hidden.filter((m) => dock.defs.has(m));

  return (
    <div
      ref={box}
      data-drop="radial"
      className={`bloom absolute inset-0 z-30 grid place-items-center overflow-hidden ${closing ? "closing" : ""}`}
      onClick={(e) => e.target === e.currentTarget && close()}
    >
      <div ref={flower} className="flower relative size-[760px] shrink-0">
        <svg viewBox="0 0 760 760" className="absolute inset-0 size-full overflow-visible" aria-hidden>
          <circle cx={CX} cy={CX} r={R2 + 9} className="rind" />
          {wedges.map((w) => (
            <path key={w.c.id} className={`wedge ${overCat === w.c.id ? "over" : ""}`} d={sector(R1, R2, w.a0, w.a1)} style={{ fill: `color-mix(in srgb, ${toneColor(w.c.tone)} 15%, var(--card))` }} />
          ))}
          <circle cx={CX} cy={CX} r={R1 - 8} className="pith" />
        </svg>
        {wedges.map((w) => {
          const [lx, ly] = pt(RL, (w.a0 + w.a1) / 2);
          return (
            <span
              key={w.c.id}
              className="absolute flex -translate-x-1/2 -translate-y-1/2 items-center gap-0.5 text-sm font-bold whitespace-nowrap"
              style={{ left: lx, top: ly, color: `color-mix(in srgb, ${toneColor(w.c.tone)} 75%, var(--foreground))` }}
            >
              <CatName name={w.c.name} editing={editCat === w.c.id} onEdited={() => setEditCat(null)} onRename={(n) => dock.renameCat(w.c.id, n)} />
              {w.mods.length === 0 && !dock.layout.cats.find((c) => c.id === w.c.id)?.mods.length && (
                <button type="button" aria-label={`删除分类「${w.c.name}」`} onClick={() => dock.deleteCat(w.c.id)} className="rounded-md p-0.5 text-muted-foreground hover:bg-card">
                  <X size={13} />
                </button>
              )}
            </span>
          );
        })}
        {wedges.flatMap((w) =>
          w.mods.map((m, k) => {
            const [x, y] = pt(RM, w.a0 + per * (k + 0.5));
            const def = dock.defs.get(m)!;
            const dim = !!term && !matches.includes(m);
            return (
              <div
                key={m}
                data-mod={m}
                title={live[m]}
                onPointerDown={(e) => pressModule(e, m, "cat", dock.move)}
                onClick={() => go(m)}
                onContextMenu={(e) => menu.at(e, m)}
                className={`group/node absolute -mt-[31px] -ml-[38px] flex w-[76px] cursor-default flex-col items-center gap-1 transition-[opacity,transform] duration-150 hover:scale-110 ${dim ? "opacity-15" : ""} ${drag?.mod === m ? "opacity-40" : ""}`}
                style={{ left: x, top: y }}
              >
                <span className={`rounded-[13px] shadow-sm ${term && !dim ? "ring-[3px] ring-[color-mix(in_srgb,var(--primary)_55%,transparent)]" : ""}`}>
                  <ModuleIcon def={def} size={42} />
                </span>
                <span className="text-xs leading-tight whitespace-nowrap">{def.label}</span>
                {dock.layout.pinned.includes(m) && <Pin size={11} className="absolute -top-1.5 left-2 text-muted-foreground" />}
                {def.assistant && <Bird size={11} className="absolute -top-1.5 right-2.5" style={{ color: "var(--module-picky)" }} />}
                <button
                  type="button"
                  aria-label={`${def.label}的菜单`}
                  onPointerDown={(e) => e.stopPropagation()}
                  onClick={(e) => {
                    e.stopPropagation();
                    menu.below(e.currentTarget, m);
                  }}
                  className="absolute -top-2 -right-0.5 grid size-5 place-items-center rounded-md bg-card text-muted-foreground opacity-0 shadow-sm group-hover/node:opacity-100"
                >
                  <Ellipsis size={13} />
                </button>
              </div>
            );
          }),
        )}
        <div className="absolute top-1/2 left-1/2 flex w-[230px] -translate-x-1/2 -translate-y-1/2 flex-col items-center gap-2">
          <span className="text-muted-foreground">{today()}</span>
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
            placeholder="去哪，或问小拾"
            spellCheck={false}
            aria-label="去哪，或问小拾"
            className="h-9 w-[196px] rounded-full border bg-background px-3.5 text-center outline-none focus:border-[color-mix(in_srgb,var(--primary)_60%,var(--border))] focus:shadow-[0_0_0_3px_color-mix(in_srgb,var(--primary)_14%,transparent)]"
          />
          {term && !first && props.onAsk && (
            <button
              type="button"
              onClick={() => {
                props.onAsk!(q.trim());
                props.onClose();
              }}
              className="flex max-w-[220px] items-center gap-1.5 rounded-control px-2 py-1 text-sm hover:bg-muted"
            >
              <Bird size={14} className="shrink-0" style={{ color: "var(--module-picky)" }} />
              <span className="truncate">问小拾「{q.trim()}」</span>
            </button>
          )}
        </div>
      </div>
      <button type="button" onClick={() => setEditCat(dock.addCat())} className="absolute bottom-4 left-[18px] flex items-center gap-1.5 rounded-control px-2 py-1 text-sm text-muted-foreground hover:bg-muted hover:text-foreground">
        <Plus size={16} />
        分类
      </button>
      <div
        data-drop="hidden"
        className={`absolute right-[18px] bottom-4 flex w-[260px] flex-col items-end gap-1.5 rounded-xl p-1.5 ${drag?.over?.zone === "hidden" ? "outline-2 outline-offset-[-2px] outline-dashed outline-primary" : ""} ${!hidden.length && !drag ? "hidden" : ""}`}
      >
        {hidden.length ? (
          <>
            {showHidden && (
              <div className="w-full rounded-xl border bg-card p-1.5">
                {hidden.map((m) => (
                  <ModuleRow key={m} id={m} dock={dock} live={live[m]} menu={menu} from="hidden" onOpen={go} dim />
                ))}
              </div>
            )}
            <button type="button" onClick={() => setShowHidden(!showHidden)} className="flex items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[13px] text-muted-foreground hover:bg-muted hover:text-foreground">
              <EyeOff size={13} />
              已隐藏 {hidden.length}
              {showHidden ? <ChevronDown size={13} /> : <ChevronUp size={13} />}
            </button>
          </>
        ) : (
          <div className="flex min-h-11 w-[200px] items-center justify-center gap-1.5 rounded-xl border-[1.5px] border-dashed text-muted-foreground">
            <EyeOff size={13} />
            隐藏
          </div>
        )}
      </div>
      {menu.open && <Menu {...menu.open} onClose={menu.close} />}
    </div>
  );
}
