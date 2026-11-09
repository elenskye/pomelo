// The 模块坞's shared pieces (the 模块坞 itself is the 贴纸簿 now, StickerBook.tsx; B3, 2026-11-08): a module's menu
// (置顶 / 隐藏 / 图标和颜色 / 移到), a category's name that renames in place, a module's row (the 已隐藏 list), and
// each module's line of live text (Rust dock.rs).
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bird, Ellipsis, Eye, EyeOff, Palette, Pin, PinOff } from "@/shared/icons";
import { markSvg, YouziBadge } from "@/shared/marks";
import { toneColor, type Dock } from "./dock";
import { pressModule, useDragNow } from "./drag";
import { LookDialog } from "./LookPicker";
import { type MenuAt } from "./Menu";
import { ModuleIcon } from "./ModuleIcon";

export const liveKey = ["dock_live"];
export const liveText = () => invoke<Record<string, string>>("dock_live");

/** 置顶 / 隐藏 / 图标和颜色 / 移到 for one module, at the pointer or under a button. Render `dialog` next to the menu. */
export function useModuleMenu(dock: Dock) {
  const [open, setOpen] = useState<MenuAt | null>(null);
  const [look, setLook] = useState<string | null>(null);
  const rows = (m: string): MenuAt["rows"] => {
    const pinned = dock.layout.pinned.includes(m),
      hidden = dock.hidden.includes(m),
      cur = dock.catOf(m)?.id;
    return [
      { label: pinned ? "取消置顶" : "置顶", icon: pinned ? <PinOff size={14} /> : <Pin size={14} />, onSelect: () => dock.togglePin(m) },
      { label: hidden ? "取消隐藏" : "隐藏", icon: hidden ? <Eye size={14} /> : <EyeOff size={14} />, onSelect: () => dock.toggleHide(m) },
      ...(m !== "youzi" ? [{ label: "图标和颜色…", icon: <Palette size={14} />, onSelect: () => setLook(m) }] : []),
      "-",
      { head: "移到" },
      ...dock.layout.cats.map((c) => ({ label: c.name, dot: toneColor(c), checked: c.id === cur, onSelect: () => dock.move(m, `cat:${c.id}`) })),
    ];
  };
  const lookDef = look ? dock.defs.get(look) : undefined;
  return {
    open,
    close: () => setOpen(null),
    dialog: lookDef ? <LookDialog def={lookDef} onClose={() => setLook(null)} /> : null,
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
        {def.assistant && (markSvg("pomelo-normal") ? <YouziBadge size={14} /> : <Bird size={12} style={{ color: "var(--module-youzi)" }} />)}
        {props.dock.layout.pinned.includes(props.id) && <Pin size={11} className="text-muted-foreground" />}
      </span>
      <span className="ml-auto min-w-0 truncate text-xs text-muted-foreground">{props.live}</span>
      <MoreButton label={def.label} onOpen={(el) => props.menu.below(el, props.id)} className="absolute top-1/2 right-1 -translate-y-1/2" />
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
