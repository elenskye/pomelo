// 模块坞 (requirements「界面外框」; route /dock, and what a new 页签 shows first): every module — built in,
// made from 五花八门 / 循序渐进, and 小拾's 助手 — with a line of live text (Rust dock.rs). The pinned row on
// top, then the categories; hidden ones fold away at the bottom. Drag to move between categories, into
// 置顶 (or the 索引签) to pin, into 已隐藏 to hide; 「⋯」 or right-click for the same; a category's 「＋」
// makes a module in it. Same layout as the 柚子瓣.
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import { Bird, ChevronDown, ChevronUp, Ellipsis, Eye, EyeOff, LayoutGrid, ListChecks, Pin, PinOff, Plus, TrendingUp, X } from "lucide-react";
import { CustomDialog } from "@/modules/custom/CustomDialog";
import type { Template } from "@/modules/custom/api";
import { PageHeader } from "@/shared/ui";
import { toneColor, useDock, type Dock } from "./dock";
import { pressModule, useDragNow } from "./drag";
import { Menu, type MenuAt } from "./Menu";
import type { ModuleDef } from "./modules";
import { useTabs } from "./tabs";

export const liveKey = ["dock_live"];
export const liveText = () => invoke<Record<string, string>>("dock_live");

/** A module's colored square with its white icon. */
export function ModuleIcon(props: { def: ModuleDef; size: number }) {
  const Icon = props.def.icon;
  return (
    <span className="grid shrink-0 place-items-center text-white" style={{ width: props.size, height: props.size, borderRadius: Math.round(props.size * 0.3), background: props.def.color }}>
      <Icon size={props.size >= 36 ? 20 : props.size >= 24 ? 15 : 12} strokeWidth={1.75} />
    </span>
  );
}

/** 置顶 / 隐藏 / 移到 for one module, at the pointer or under a button. */
export function useModuleMenu(dock: Dock) {
  const [open, setOpen] = useState<MenuAt | null>(null);
  const rows = (m: string): MenuAt["rows"] => {
    const pinned = dock.layout.pinned.includes(m),
      hidden = dock.hidden.includes(m),
      cur = dock.catOf(m)?.id;
    return [
      { label: pinned ? "取消置顶" : "置顶", icon: pinned ? <PinOff size={14} /> : <Pin size={14} />, onSelect: () => dock.togglePin(m) },
      { label: hidden ? "取消隐藏" : "隐藏", icon: hidden ? <Eye size={14} /> : <EyeOff size={14} />, onSelect: () => dock.toggleHide(m) },
      "-",
      { head: "移到" },
      ...dock.layout.cats.map((c) => ({ label: c.name, dot: toneColor(c.tone), checked: c.id === cur, onSelect: () => dock.move(m, `cat:${c.id}`) })),
    ];
  };
  return {
    open,
    close: () => setOpen(null),
    at: (e: React.MouseEvent, m: string) => {
      e.preventDefault();
      setOpen({ x: e.clientX, y: e.clientY, rows: rows(m) });
    },
    below: (el: HTMLElement, m: string) => {
      const r = el.getBoundingClientRect();
      setOpen({ x: r.left, y: r.bottom + 4, rows: rows(m) });
    },
  };
}
type ModuleMenu = ReturnType<typeof useModuleMenu>;

function MoreButton(props: { label: string; onOpen: (el: HTMLElement) => void; className: string }) {
  return (
    <button
      type="button"
      aria-label={`${props.label}的菜单`}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        props.onOpen(e.currentTarget);
      }}
      className={`grid size-[22px] place-items-center rounded-md text-muted-foreground opacity-0 hover:bg-black/5 hover:text-foreground group-hover/m:opacity-100 dark:hover:bg-white/5 ${props.className}`}
    >
      <Ellipsis size={14} />
    </button>
  );
}

/** One module in a category (or 已隐藏): icon, name, pin mark, live text. */
export function ModuleRow(props: { id: string; dock: Dock; live?: string; menu: ModuleMenu; from: string; onOpen: (id: string) => void; dim?: boolean }) {
  const def = props.dock.defs.get(props.id);
  const drag = useDragNow();
  if (!def) return null;
  return (
    <div
      data-mod={props.id}
      role="link"
      tabIndex={0}
      onPointerDown={(e) => pressModule(e, props.id, props.from, props.dock.move)}
      onClick={() => props.onOpen(props.id)}
      onKeyDown={(e) => e.key === "Enter" && props.onOpen(props.id)}
      onContextMenu={(e) => props.menu.at(e, props.id)}
      className={`group/m relative flex min-w-0 cursor-default items-center gap-2.5 rounded-control py-1 pr-8 pl-1.5 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring ${props.dim ? "opacity-70" : ""} ${drag?.mod === props.id ? "opacity-40" : ""}`}
    >
      <ModuleIcon def={def} size={26} />
      <span className="flex shrink-0 items-center gap-1 whitespace-nowrap">
        {def.label}
        {def.assistant && <Bird size={12} style={{ color: "var(--module-picky)" }} />}
        {props.dock.layout.pinned.includes(props.id) && <Pin size={11} className="text-muted-foreground" />}
      </span>
      <span className="ml-auto min-w-0 truncate text-xs text-muted-foreground">{props.live}</span>
      <MoreButton label={def.label} onOpen={(el) => props.menu.below(el, props.id)} className="absolute top-1/2 right-1 -translate-y-1/2" />
    </div>
  );
}

function Tile(props: { id: string; dock: Dock; live?: string; menu: ModuleMenu; onOpen: (id: string) => void }) {
  const def = props.dock.defs.get(props.id);
  const drag = useDragNow();
  if (!def) return null;
  return (
    <div
      data-mod={props.id}
      role="link"
      tabIndex={0}
      onPointerDown={(e) => pressModule(e, props.id, "pinned", props.dock.move)}
      onClick={() => props.onOpen(props.id)}
      onKeyDown={(e) => e.key === "Enter" && props.onOpen(props.id)}
      onContextMenu={(e) => props.menu.at(e, props.id)}
      className={`group/m relative flex w-[112px] cursor-default flex-col items-center gap-0.5 rounded-[14px] border bg-card px-1.5 pt-3 pb-2.5 text-center outline-none transition-[transform,box-shadow] duration-150 hover:-translate-y-px hover:shadow-sm focus-visible:ring-2 focus-visible:ring-ring max-[1099px]:w-[94px] ${drag?.mod === props.id ? "opacity-40" : ""}`}
    >
      {def.assistant && <Bird size={12} className="absolute top-1.5 left-2" style={{ color: "var(--module-picky)" }} />}
      <span className="max-[1099px]:hidden">
        <ModuleIcon def={def} size={40} />
      </span>
      <span className="hidden max-[1099px]:block">
        <ModuleIcon def={def} size={32} />
      </span>
      <span className="mt-1 max-w-full truncate text-[13px] leading-tight font-bold">{def.label}</span>
      <span className="max-w-full truncate text-[11px] leading-tight text-muted-foreground">{props.live ?? "\u00a0"}</span>
      <MoreButton label={def.label} onOpen={(el) => props.menu.below(el, props.id)} className="absolute top-1 right-1" />
    </div>
  );
}

/** A category's name: click to rename in place (↩ or leaving saves, Esc keeps the old one). */
export function CatName(props: { name: string; editing?: boolean; onEdited?: () => void; onRename: (name: string) => void; className?: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!props.editing || !ref.current) return;
    ref.current.focus();
    getSelection()?.selectAllChildren(ref.current);
    props.onEdited?.();
  }, [props.editing]); // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <span
      ref={ref}
      contentEditable="plaintext-only"
      suppressContentEditableWarning
      spellCheck={false}
      role="textbox"
      aria-label="分类名"
      onBlur={(e) => {
        const v = e.currentTarget.textContent ?? "";
        if (v.trim() && v.trim() !== props.name) props.onRename(v);
        else e.currentTarget.textContent = props.name;
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          e.currentTarget.blur();
        } else if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          e.currentTarget.textContent = props.name;
          e.currentTarget.blur();
        }
      }}
      className={`cursor-text rounded px-1 outline-none hover:bg-card focus:bg-card ${props.className ?? ""}`}
    >
      {props.name}
    </span>
  );
}

export default function DockPage() {
  const dock = useDock();
  const tabs = useTabs();
  const drag = useDragNow();
  const live = useQuery({ queryKey: liveKey, queryFn: liveText, staleTime: 0 }).data ?? {};
  const menu = useModuleMenu(dock);
  const [showHidden, setShowHidden] = useState(false);
  const [editCat, setEditCat] = useState<string | null>(null);
  const [making, setMaking] = useState<{ template: Template; cat: string } | null>(null);
  const [newMenu, setNewMenu] = useState<MenuAt | null>(null);
  const open = (id: string) => tabs.open(`/${id}`);
  const over = (zone: string) => (drag?.over?.zone === zone ? "outline-2 outline-offset-2 outline-dashed outline-primary" : "");
  const hidden = dock.hidden.filter((m) => dock.defs.has(m));

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader icon={LayoutGrid} color="var(--muted-foreground)" title="模块坞" />
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="flex flex-col gap-3.5 px-8 pt-1 pb-8">
          <section className="flex flex-col gap-2">
            <div className="flex items-center gap-1.5 text-[13px] text-muted-foreground">
              <Pin size={13} />
              置顶
            </div>
            <div data-drop="pinned" className={`flex min-h-[90px] flex-wrap gap-2.5 rounded-xl ${over("pinned")}`}>
              {dock.pinned.map((m) => (
                <Tile key={m} id={m} dock={dock} live={live[m]} menu={menu} onOpen={open} />
              ))}
            </div>
          </section>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] items-start gap-3">
            {dock.layout.cats.map((c) => {
              const mods = c.mods.filter((m) => !dock.isHidden(m));
              return (
                <section key={c.id} data-drop={`cat:${c.id}`} className={`group/cat flex min-h-[52px] flex-col gap-px rounded-card border bg-card px-1.5 pt-1.5 pb-2 ${over(`cat:${c.id}`)}`}>
                  <div className="flex min-h-[30px] items-center gap-1.5 pt-0.5 pr-0.5 pb-1 pl-1.5">
                    <span className="size-2 shrink-0 rounded-full" style={{ background: toneColor(c.tone) }} />
                    <CatName name={c.name} editing={editCat === c.id} onEdited={() => setEditCat(null)} onRename={(n) => dock.renameCat(c.id, n)} className="font-bold" />
                    <span className="text-xs text-muted-foreground">{mods.length || ""}</span>
                    <span className="flex-1" />
                    {!c.mods.length && (
                      <button type="button" aria-label={`删除分类「${c.name}」`} onClick={() => dock.deleteCat(c.id)} className="grid size-[26px] place-items-center rounded-md text-muted-foreground opacity-0 group-hover/cat:opacity-100 hover:bg-muted hover:text-foreground">
                        <X size={13} />
                      </button>
                    )}
                    <button
                      type="button"
                      aria-label={`在「${c.name}」里新建模块`}
                      onClick={(e) => {
                        const r = e.currentTarget.getBoundingClientRect();
                        setNewMenu({
                          x: r.left,
                          y: r.bottom + 4,
                          rows: [
                            { label: "五花八门", icon: <ListChecks size={14} />, onSelect: () => setMaking({ template: "list", cat: c.id }) },
                            { label: "循序渐进", icon: <TrendingUp size={14} />, onSelect: () => setMaking({ template: "progress", cat: c.id }) },
                          ],
                        });
                      }}
                      className="grid size-[26px] place-items-center rounded-md text-muted-foreground opacity-0 group-hover/cat:opacity-100 hover:bg-muted hover:text-foreground"
                    >
                      <Plus size={13} />
                    </button>
                  </div>
                  {mods.map((m) => (
                    <ModuleRow key={m} id={m} dock={dock} live={live[m]} menu={menu} from="cat" onOpen={open} />
                  ))}
                </section>
              );
            })}
            <button type="button" onClick={() => setEditCat(dock.addCat())} className="flex min-h-[52px] items-center justify-center gap-1.5 rounded-card border-[1.5px] border-dashed text-muted-foreground hover:bg-muted/60 hover:text-foreground">
              <Plus size={16} />
              分类
            </button>
          </div>
          <div data-drop="hidden" className={`flex flex-col gap-1.5 rounded-xl ${over("hidden")}`}>
            {hidden.length ? (
              <>
                <button type="button" onClick={() => setShowHidden(!showHidden)} className="flex items-center gap-1.5 self-start rounded-md px-1.5 py-0.5 text-[13px] text-muted-foreground hover:bg-muted hover:text-foreground">
                  <EyeOff size={13} />
                  已隐藏 {hidden.length}
                  {showHidden ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
                </button>
                {showHidden && (
                  <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-x-3 gap-y-0.5">
                    {hidden.map((m) => (
                      <ModuleRow key={m} id={m} dock={dock} live={live[m]} menu={menu} from="hidden" onOpen={open} dim />
                    ))}
                  </div>
                )}
              </>
            ) : (
              drag && (
                <div className="flex min-h-[46px] items-center justify-center gap-1.5 rounded-xl border-[1.5px] border-dashed text-muted-foreground">
                  <EyeOff size={13} />
                  隐藏
                </div>
              )
            )}
          </div>
        </div>
      </div>
      {menu.open && <Menu {...menu.open} onClose={menu.close} />}
      {newMenu && <Menu {...newMenu} onClose={() => setNewMenu(null)} />}
      {making && <CustomDialog template={making.template} onClose={() => setMaking(null)} onMade={(key) => dock.place(key, making.cat)} />}
    </div>
  );
}
