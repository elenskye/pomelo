// 贴纸簿 (requirements「界面外框」: the 模块坞 became a sticker book, 2026-11-08; visual-atelier's frame prototype,
// board Book): a striped paper book slides up from the bottom of the desk, tilted a little. Each category's name is
// written on a strip of tape; its modules are stickers, five to a row. A page already on the desk leaves a dashed
// gap saying「在桌上」. Click a sticker: it peels off and flies away, and its page opens; drag it out over the book's
// top edge to put it on the desk too; drop it on another category to move it there. Right-click a sticker for
// 置顶 / 隐藏 / 图标和颜色 / 移到; right-click a category's tape to rename it, make a module in it or delete it.
// Typing finds a page (↩ opens the first; nothing matches → ask 柚子). Esc or a click beside the book closes it.
// Motion: ds.css (book-open, sticker-peel).
import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ListChecks, Pencil, TrendingUp, Trash2 } from "lucide-react";
import { CustomDialog } from "@/modules/custom/CustomDialog";
import type { Template } from "@/modules/custom/api";
import { reducedMotion } from "@/shared/hand";
import { HandIcon, Tape } from "@/shared/handParts";
import { YouziBadge } from "@/shared/marks";
import { CatName, liveKey, liveText, ModuleRow, useModuleMenu } from "./DockPage";
import { useDock } from "./dock";
import { Menu, type MenuAt } from "./Menu";
import { ModuleIcon } from "./ModuleIcon";
import { splitPath } from "./modules";
import { PomeloFace } from "./Pomelo";
import { useTabs } from "./tabs";

const TILT = [-3, 2, -2, 3, -4, 2];
const STICK_TILT = [-6, 5, -3, 7, -5, 4];
const CLOSE_MS = 320;

export function StickerBook(props: { onClose: () => void; onAsk: ((text: string) => void) | null }) {
  const dock = useDock();
  const tabs = useTabs();
  const live = useQuery({ queryKey: liveKey, queryFn: liveText, staleTime: 0 }).data ?? {};
  const menu = useModuleMenu(dock);
  const [q, setQ] = useState("");
  const [closing, setClosing] = useState(false);
  const [wow, setWow] = useState(true);
  const [showHidden, setShowHidden] = useState(false);
  const [editCat, setEditCat] = useState<string | null>(null);
  const [catMenu, setCatMenu] = useState<MenuAt | null>(null);
  const [making, setMaking] = useState<{ template: Template; cat: string } | null>(null);
  const bookRef = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);

  const close = () => {
    if (closing) return;
    if (reducedMotion()) return props.onClose();
    setClosing(true);
    setTimeout(props.onClose, CLOSE_MS);
  };
  const go = (m: string) => tabs.open(`/${m}`);
  useEffect(() => {
    input.current?.focus({ preventScroll: true });
    const t = setTimeout(() => setWow(false), 900);
    return () => clearTimeout(t);
  }, []);
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

  const onDesk = new Set(tabs.tabs.map((t) => splitPath(t.path).module));
  const cats = dock.layout.cats.map((c) => ({ c, mods: c.mods.filter((m) => !dock.isHidden(m)) }));
  const term = q.trim().toLowerCase();
  const matches = term ? cats.flatMap((x) => x.mods).filter((m) => dock.defs.get(m)!.label.toLowerCase().includes(term)) : [];
  const hidden = dock.hidden.filter((m) => dock.defs.has(m));
  const state = closing ? "is-closing" : "is-open";

  /** Press a sticker: it lifts; drag to carry it (out over the book's top: open; onto a category: move); let go without dragging: it peels off and flies away. */
  const press = (e: React.PointerEvent<HTMLDivElement>, m: string, cat: string) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    el.classList.add("is-peeling");
    const x0 = e.clientX, y0 = e.clientY;
    let drag = false;
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - x0, dy = ev.clientY - y0;
      if (!drag && Math.abs(dx) + Math.abs(dy) < 5) return;
      drag = true;
      el.classList.add("is-held");
      el.style.transform = `translate(${dx}px,${dy}px) rotate(${(dx * 0.05).toFixed(1)}deg) scale(1.12)`;
    };
    const up = (ev: PointerEvent) => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      el.classList.remove("is-peeling", "is-held");
      if (drag) {
        const top = bookRef.current?.getBoundingClientRect().top ?? 0;
        if (ev.clientY < top + 10) {
          el.style.opacity = "0";
          return go(m);
        }
        el.style.transform = "";
        el.style.visibility = "hidden";
        const target = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>("[data-cat]");
        el.style.visibility = "";
        if (target && target.dataset.cat !== cat) dock.move(m, `cat:${target.dataset.cat}`);
        return;
      }
      if (reducedMotion()) return go(m);
      el.classList.add("is-flyoff");
      setTimeout(() => go(m), 420);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };

  const openCatMenu = (e: React.MouseEvent, id: string, empty: boolean) => {
    e.preventDefault();
    setCatMenu({
      x: e.clientX,
      y: e.clientY,
      rows: [
        { label: "改名", icon: <Pencil size={14} />, onSelect: () => setEditCat(id) },
        { label: "在这里新建「五花八门」", icon: <ListChecks size={14} />, onSelect: () => setMaking({ template: "list", cat: id }) },
        { label: "在这里新建「循序渐进」", icon: <TrendingUp size={14} />, onSelect: () => setMaking({ template: "progress", cat: id }) },
        ...(empty ? ["-" as const, { label: "删除这个分类", icon: <Trash2 size={14} />, onSelect: () => dock.deleteCat(id) }] : []),
      ],
    });
  };

  return (
    <div className={`bk pm-scrim ${state}`} role="dialog" aria-label="柚子的贴纸簿" onClick={(e) => e.target === e.currentTarget && close()}>
      <div ref={bookRef} className={`book pm-book ${state}`}>
        <div className="bookpaper" data-hand="frame" data-r="18" data-amp="1.1" data-sw="2" data-seed="820" data-tilt="-1.2" />
        <div className="bin">
          <div className="bhead">
            <PomeloFace face={wow ? "wow" : "idle"} size={64} />
            <div>
              <div className="bt">柚子的贴纸簿</div>
              <div className="bsub">撕一张贴纸，就去那里 · 拖出簿子放到桌上也行</div>
            </div>
            <label className="pm-field bfield" data-hand="field" data-seed="821" data-r="12" data-fill="paper">
              <span className="pm-field-icon" style={{ order: -1 }}>
                <HandIcon name="search" size={18} seed={952} />
              </span>
              <input
                ref={input}
                value={q}
                onChange={(e) => setQ(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key !== "Enter") return;
                  e.preventDefault();
                  if (matches[0]) go(matches[0]);
                  else if (term && props.onAsk) {
                    props.onAsk(q.trim());
                    props.onClose();
                  }
                }}
                placeholder="去哪，或问柚子"
                aria-label="去哪，或问柚子"
                spellCheck={false}
              />
            </label>
          </div>
          {term && !matches.length && props.onAsk && (
            <button
              type="button"
              className="bask pm-press"
              onClick={() => {
                props.onAsk!(q.trim());
                props.onClose();
              }}
            >
              <HandIcon name="sparkle" size={14} seed={953} />
              <span>没有这一页，问柚子「{q.trim()}」</span>
            </button>
          )}
          <div className="bgrid">
            {cats.map(({ c, mods }, ci) => (
              <section key={c.id} className="bcat" data-cat={c.id}>
                <div className="blab" onContextMenu={(e) => openCatMenu(e, c.id, !c.mods.length)}>
                  <Tape kind={ci % 2 ? "yuzu" : "mint"} rot={TILT[ci % TILT.length]} />
                  <span className="tl">
                    <CatName name={c.name} editing={editCat === c.id} onEdited={() => setEditCat(null)} onRename={(n) => dock.renameCat(c.id, n)} />
                  </span>
                </div>
                <div className="bcells">
                  {mods.map((m, j) => {
                    const def = dock.defs.get(m)!;
                    const hit = !!term && matches.includes(m);
                    return (
                      <div
                        key={m}
                        data-stk={m}
                        role="button"
                        tabIndex={0}
                        aria-label={def.label}
                        title={live[m] || undefined}
                        onPointerDown={(e) => press(e, m, c.id)}
                        onKeyDown={(e) => e.key === "Enter" && go(m)}
                        onContextMenu={(e) => menu.at(e, m)}
                        className={`stk pm-peel ${onDesk.has(m) ? "used" : ""} ${term && !hit ? "dim" : ""} ${hit ? "hit" : ""}`}
                      >
                        <span className="ghost" />
                        <span className="stkart pm-peel-art">
                          <span style={{ transform: `rotate(${STICK_TILT[(ci + j) % STICK_TILT.length]}deg)`, display: "inline-flex" }}>
                            <ModuleIcon def={def} size={66} />
                          </span>
                        </span>
                        <span className="stklab">
                          {def.label}
                          {def.assistant && <YouziBadge size={14} />}
                        </span>
                        <span className="usedtag">在桌上</span>
                      </div>
                    );
                  })}
                </div>
              </section>
            ))}
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
      {catMenu && <Menu {...catMenu} onClose={() => setCatMenu(null)} />}
      {making && <CustomDialog template={making.template} onClose={() => setMaking(null)} onMade={(key) => dock.place(key, making.cat)} />}
    </div>
  );
}
