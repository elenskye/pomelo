// The 索引签 on the paper's right edge (docs/design.md「外框」): 首页, 模块坞, then the pinned modules in
// their colors (drag to reorder, drop a module here to pin it, right-click for its menu). The current
// page's one turns paper-colored and reaches into the page. Narrow windows show icons only.
import { useLocation } from "react-router";
import { useDock } from "./dock";
import { pressModule, useDragNow } from "./drag";
import { useModuleMenu } from "./DockPage";
import { Menu } from "./Menu";
import { moduleById, splitPath } from "./modules";
import { useTabs } from "./tabs";

function IndexTab(props: { id: string; on: boolean; pinned?: boolean; dot?: boolean; onPress?: (e: React.PointerEvent) => void; onMenu?: (e: React.MouseEvent) => void; faded?: boolean }) {
  const tabs = useTabs();
  const m = moduleById(props.id);
  const Icon = m.icon;
  return (
    <div
      data-mod={props.pinned ? props.id : undefined}
      role="link"
      tabIndex={0}
      title={m.label}
      aria-current={props.on ? "page" : undefined}
      onPointerDown={props.onPress}
      onClick={() => tabs.open(`/${props.id}`)}
      onKeyDown={(e) => e.key === "Enter" && tabs.open(`/${props.id}`)}
      onContextMenu={props.onMenu}
      style={props.on ? undefined : { background: `color-mix(in srgb, ${m.color} 16%, var(--card))` }}
      className={`relative -ml-0.5 flex h-[31px] shrink-0 cursor-default items-center gap-1.5 rounded-r-[10px] border border-l-0 pr-2.5 pl-3 text-[13px] whitespace-nowrap outline-none transition-[transform,margin] duration-150 focus-visible:ring-2 focus-visible:ring-ring max-[1099px]:justify-center max-[1099px]:pr-1.5 max-[1099px]:pl-2 ${
        props.on ? "z-[3] -ml-1.5 border-border bg-background pl-4 font-bold shadow-[2px_1px_4px_rgba(0,0,0,0.06)]" : "border-transparent hover:translate-x-[3px]"
      } ${props.faded ? "opacity-40" : ""}`}
    >
      <Icon size={13} strokeWidth={1.75} className="shrink-0" style={{ color: m.color }} />
      <span className="min-w-0 truncate max-[1099px]:hidden">{m.label}</span>
      {props.dot && <span aria-label="有新内容" className="size-1.5 shrink-0 rounded-full bg-primary max-[1099px]:absolute max-[1099px]:top-1 max-[1099px]:right-1" />}
    </div>
  );
}

export function IndexTabs(props: { unseen: boolean }) {
  const dock = useDock();
  const drag = useDragNow();
  const menu = useModuleMenu(dock);
  const here = splitPath(useLocation().pathname).module;
  const over = drag?.over?.zone === "pinned";
  return (
    <nav
      aria-label="索引签"
      data-drop="pinned"
      // reaches 6px under the paper so the current tab can join it (the others tuck under its edge)
      className={`-ml-1.5 flex w-[126px] shrink-0 flex-col gap-[3px] overflow-y-auto py-2.5 pr-1 pl-1.5 max-[1099px]:w-[56px] ${over ? "rounded-r-xl outline-2 outline-offset-[-4px] outline-dashed outline-primary" : ""}`}
    >
      <IndexTab id="" on={here === ""} />
      <IndexTab id="dock" on={here === "dock"} />
      <div className="h-2.5 shrink-0" />
      {dock.pinned.map((id) => (
        <IndexTab
          key={id}
          id={id}
          pinned
          on={here === id}
          dot={id === "picky" && props.unseen}
          faded={drag?.mod === id}
          onPress={(e) => pressModule(e, id, "pinned", dock.move)}
          onMenu={(e) => menu.at(e, id)}
        />
      ))}
      {menu.open && <Menu {...menu.open} onClose={menu.close} />}
    </nav>
  );
}
