import { NavLink } from "react-router";
import { Settings, type LucideIcon } from "lucide-react";
import { MODULES } from "./modules";

function NavItem(props: { to: string; label: string; icon: LucideIcon; color?: string }) {
  const Icon = props.icon;
  return (
    <NavLink
      to={props.to}
      className={({ isActive }) =>
        `flex items-center gap-2.5 rounded-control px-2.5 py-1.5 transition-colors duration-150 ${
          isActive ? "bg-accent text-accent-foreground" : "hover:bg-black/5 dark:hover:bg-white/5"
        }`
      }
    >
      <Icon size={16} strokeWidth={1.5} style={{ color: props.color }} />
      {props.label}
    </NavLink>
  );
}

export function Sidebar(props: { isDev?: boolean }) {
  return (
    <aside className="flex w-[220px] shrink-0 flex-col border-r bg-sidebar px-3 pb-3">
      <div data-tauri-drag-region className="h-12 shrink-0" />
      <div data-tauri-drag-region className="mb-3 px-2.5 text-base font-bold">
        柚儿园
      </div>
      <nav className="flex flex-col gap-0.5">
        {MODULES.map((m) => (
          <NavItem key={m.id} to={`/${m.id}`} label={m.label} icon={m.icon} color={m.color} />
        ))}
      </nav>
      <div data-tauri-drag-region className="flex-1" />
      <nav className="flex flex-col gap-0.5">
        <NavItem to="/settings" label="设置" icon={Settings} color="var(--muted-foreground)" />
      </nav>
      {props.isDev && (
        <div className="mt-2 px-2.5 text-xs text-muted-foreground">开发版 · 使用测试数据</div>
      )}
    </aside>
  );
}
