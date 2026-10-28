// The top bar across the window (docs/design.md「外框」): the traffic lights (macOS draws them), the pomelo,
// ‹ ›, the 页签 and「＋」; on the right the date, search (⌘K), 设置 and 小拾's round button (⌘J).
// Everything sits on the traffic lights' centre line. Right-click ‹ › lists the steps that way.
import { useState } from "react";
import { Bird, ChevronLeft, ChevronRight, Plus, Search, Settings, X } from "lucide-react";
import { Menu, type MenuAt } from "./Menu";
import { moduleById, pageName, splitPath } from "./modules";
import { PomeloButton } from "./Pomelo";
import { useTabs } from "./tabs";

const dateText = () => {
  const d = new Date();
  return `${d.getMonth() + 1} 月 ${d.getDate()} 日 · 周${"日一二三四五六"[d.getDay()]}`;
};

const iconButton =
  "grid size-[30px] shrink-0 place-items-center rounded-control text-muted-foreground outline-none transition-colors duration-150 hover:bg-black/5 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-30 dark:hover:bg-white/[0.06]";

export function TopBar(props: {
  isDev?: boolean;
  bloomOpen: boolean;
  onPomelo: (el: HTMLElement) => void;
  onSearch?: () => void;
  onChat?: () => void;
  chatOpen: boolean;
  unseen: boolean;
}) {
  const tabs = useTabs();
  const [menu, setMenu] = useState<MenuAt | null>(null);
  const trail = (dir: -1 | 1) => (e: React.MouseEvent) => {
    e.preventDefault();
    const steps = tabs.trail(dir);
    if (!steps.length) return;
    const r = e.currentTarget.getBoundingClientRect();
    setMenu({
      x: r.left,
      y: r.bottom + 4,
      rows: steps.map((s) => {
        const m = moduleById(splitPath(s.path).module);
        const Icon = m.icon;
        return { label: pageName(s.path), icon: <Icon size={14} style={{ color: m.color }} />, onSelect: () => tabs.jump(s.index) };
      }),
    });
  };
  return (
    <header data-tauri-drag-region className="relative z-40 flex h-[51px] shrink-0 items-center gap-1.5 border-b bg-sidebar pr-3 pl-[90px]">
      <PomeloButton open={props.bloomOpen} onClick={props.onPomelo} />
      <div className="flex shrink-0 items-center">
        <button type="button" aria-label="后退" title="后退" disabled={!tabs.canBack} onClick={tabs.back} onContextMenu={trail(-1)} className={`${iconButton} w-7`}>
          <ChevronLeft size={16} strokeWidth={1.75} />
        </button>
        <button type="button" aria-label="前进" title="前进" disabled={!tabs.canForward} onClick={tabs.forward} onContextMenu={trail(1)} className={`${iconButton} w-7`}>
          <ChevronRight size={16} strokeWidth={1.75} />
        </button>
      </div>
      <span className="mx-1 h-[18px] w-px shrink-0 bg-border" />
      <div role="tablist" aria-label="页签" data-tauri-drag-region className="flex h-full min-w-0 flex-1 items-center gap-1 overflow-hidden">
        {tabs.tabs.map((t, i) => {
          const m = moduleById(splitPath(t.path).module);
          const Icon = m.icon;
          const on = i === tabs.active;
          const name = pageName(t.path);
          return (
            <div
              key={t.id}
              role="tab"
              aria-selected={on}
              tabIndex={0}
              title={name}
              onClick={() => tabs.select(i)}
              onKeyDown={(e) => e.key === "Enter" && tabs.select(i)}
              onAuxClick={(e) => e.button === 1 && tabs.close(i)}
              className={`group/tab flex h-[30px] max-w-[210px] min-w-0 shrink cursor-default items-center gap-1.5 rounded-control pr-1 pl-2.5 whitespace-nowrap outline-none focus-visible:ring-2 focus-visible:ring-ring ${
                on ? "bg-background text-foreground shadow-[0_1px_3px_rgba(43,40,36,0.1),0_0_0_1px_var(--border)]" : "text-muted-foreground hover:bg-black/[0.04] dark:hover:bg-white/5"
              }`}
            >
              <Icon size={13} strokeWidth={1.75} className="shrink-0" style={{ color: m.color }} />
              <span className="min-w-0 truncate">{name}</span>
              <button
                type="button"
                aria-label={`关闭「${name}」`}
                onClick={(e) => {
                  e.stopPropagation();
                  tabs.close(i);
                }}
                className={`grid shrink-0 place-items-center rounded-[5px] p-0.5 hover:bg-black/[0.07] hover:opacity-100 dark:hover:bg-white/10 ${on ? "opacity-55" : "opacity-0 group-hover/tab:opacity-55"}`}
              >
                <X size={13} />
              </button>
            </div>
          );
        })}
        <button type="button" aria-label="新页签" title="新页签" onClick={tabs.newTab} className={iconButton}>
          <Plus size={16} strokeWidth={1.75} />
        </button>
      </div>
      {props.isDev && <span className="shrink-0 rounded-full border px-2 text-xs text-muted-foreground">开发版</span>}
      <span className="shrink-0 px-1 text-[13px] whitespace-nowrap text-muted-foreground max-[1099px]:hidden">{dateText()}</span>
      {props.onSearch && (
        <button
          type="button"
          onClick={props.onSearch}
          className="flex h-[30px] w-[clamp(120px,18vw,240px)] min-w-0 shrink items-center gap-2 rounded-[9px] border bg-card px-2.5 text-muted-foreground outline-none hover:border-[color-mix(in_srgb,var(--muted-foreground)_40%,var(--border))] focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Search size={15} strokeWidth={1.75} className="shrink-0" />
          <span className="truncate">搜索</span>
        </button>
      )}
      <button type="button" aria-label="设置" title="设置" onClick={() => tabs.open("/settings")} className={iconButton}>
        <Settings size={16} strokeWidth={1.75} />
      </button>
      <button
        type="button"
        aria-label={props.chatOpen ? "收起小拾" : "问小拾"}
        title={props.chatOpen ? "收起小拾" : "问小拾"}
        aria-pressed={props.chatOpen}
        disabled={!props.onChat}
        onClick={props.onChat}
        className="relative grid size-[30px] shrink-0 place-items-center rounded-full text-white shadow-sm outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40"
        style={{ background: "radial-gradient(circle at 35% 30%, #f0a35c, var(--module-picky))" }}
      >
        <Bird size={16} strokeWidth={1.75} />
        {props.unseen && <span aria-label="有新内容" className="absolute -top-0.5 -right-0.5 size-2 rounded-full border border-sidebar bg-primary" />}
      </button>
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
    </header>
  );
}
