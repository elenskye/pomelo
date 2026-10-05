// App sidebar: modules (drag a row to reorder; order kept in settings) + 设置. Collapses to an
// icon strip (names show on hover); the state is kept in settings too.
import { NavLink } from "react-router";
import { PanelLeftClose, PanelLeftOpen, Settings, type LucideIcon } from "lucide-react";
import { isBoolean, isStringArray, useSetting } from "@/shared/settings";
import { useSortable } from "@/shared/sortable";
import { MODULES, type ModuleDef } from "./modules";

const COLLAPSED = "ui.sidebar_collapsed";
const ORDER = "ui.module_order";

/** Saved order first (unknown ids dropped), then modules added since. */
export function orderModules(order: string[]): ModuleDef[] {
  const known = order.map((id) => MODULES.find((m) => m.id === id)).filter((m): m is ModuleDef => !!m);
  return [...known, ...MODULES.filter((m) => !known.includes(m))];
}

function NavItem(props: { to: string; label: string; icon: LucideIcon; color?: string; collapsed: boolean }) {
  const Icon = props.icon;
  return (
    <NavLink
      to={props.to}
      aria-label={props.collapsed ? props.label : undefined}
      className={({ isActive }) =>
        `group/nav relative flex items-center gap-2.5 rounded-control py-1.5 transition-colors duration-150 ${props.collapsed ? "justify-center px-0" : "px-2.5"} ${
          isActive ? "bg-accent text-accent-foreground" : "hover:bg-black/5 dark:hover:bg-white/5"
        }`
      }
    >
      <Icon size={16} strokeWidth={1.5} style={{ color: props.color }} className="shrink-0" />
      {props.collapsed ? (
        <span
          role="tooltip"
          className="pointer-events-none invisible absolute top-1/2 left-full z-30 ml-2 -translate-y-1/2 rounded-control border bg-popover px-2 py-1 text-sm whitespace-nowrap text-foreground opacity-0 shadow-lg transition-opacity duration-150 group-hover/nav:visible group-hover/nav:opacity-100 group-focus-visible/nav:visible group-focus-visible/nav:opacity-100"
        >
          {props.label}
        </span>
      ) : (
        <span className="truncate">{props.label}</span>
      )}
    </NavLink>
  );
}

export function Sidebar(props: { isDev?: boolean }) {
  const [collapsed, setCollapsed] = useSetting(COLLAPSED, false, isBoolean);
  const [order, setOrder] = useSetting<string[]>(ORDER, [], isStringArray);
  const modules = orderModules(order);
  const sort = useSortable<HTMLElement>((ids) => setOrder(ids.map((i) => modules[i].id)));
  const Toggle = collapsed ? PanelLeftOpen : PanelLeftClose;
  const toggle = (
    <button
      type="button"
      aria-label={collapsed ? "展开侧栏" : "收起侧栏"}
      title={collapsed ? "展开侧栏" : "收起侧栏"}
      onClick={() => setCollapsed(!collapsed)}
      className="rounded-control p-1.5 text-muted-foreground transition-colors duration-150 outline-none hover:bg-black/5 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring dark:hover:bg-white/5"
    >
      <Toggle size={16} strokeWidth={1.5} />
    </button>
  );
  return (
    <aside
      className={`flex shrink-0 flex-col border-r bg-sidebar pb-3 transition-[width] duration-150 ${collapsed ? "w-[60px] px-2" : "w-[220px] px-3"}`}
    >
      <div data-tauri-drag-region className="h-12 shrink-0" />
      {collapsed ? (
        <div className="mb-3 flex justify-center">{toggle}</div>
      ) : (
        <div data-tauri-drag-region className="mb-3 flex items-center justify-between pl-2.5">
          <span className="pointer-events-none text-base font-bold">柚儿园</span>
          {toggle}
        </div>
      )}
      <nav ref={sort.listRef} className="flex flex-col gap-0.5">
        {modules.map((m, i) => (
          <div key={m.id} {...sort.item(i)} {...sort.handle(i)} className="rounded-control bg-sidebar">
            <NavItem to={`/${m.id}`} label={m.label} icon={m.icon} color={m.color} collapsed={collapsed} />
          </div>
        ))}
      </nav>
      <div data-tauri-drag-region className="flex-1" />
      <nav className="flex flex-col gap-0.5">
        <NavItem to="/settings" label="设置" icon={Settings} color="var(--muted-foreground)" collapsed={collapsed} />
      </nav>
      {props.isDev && (
        <div className={`mt-2 text-xs text-muted-foreground ${collapsed ? "text-center" : "px-2.5"}`}>{collapsed ? "开发" : "开发版 · 使用测试数据"}</div>
      )}
    </aside>
  );
}
