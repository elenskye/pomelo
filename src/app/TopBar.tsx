// The top bar across the window (docs/design.md「外框」, the B3 desk of visual-atelier's frame prototype): the traffic
// lights (macOS draws them), the pomelo (the 柚子瓣), ‹ › (the pages you went through; right-click lists them),
// 「回桌面」 on a strip of tape when a page is in front; on the right「桌上开着」— a small sticker for each open page
// (click to bring it to the front, × to close it, its name hangs under it) —「＋」 (the 贴纸簿), the date, 找东西 (⌘K),
// 设置 and 柚子's button (⌘J). Icon-only buttons have a tooltip on a string instead of a title.
import { useEffect, useRef, useState } from "react";
import { Bird } from "lucide-react";
import { HandIcon, Tape, Tip } from "@/shared/handParts";
import { reducedMotion } from "@/shared/hand";
import { Mark, markSvg } from "@/shared/marks";
import { Menu, type MenuAt } from "./Menu";
import { ModuleIcon } from "./ModuleIcon";
import { moduleById, pageName, splitPath } from "./modules";
import { PomeloButton, type Face } from "./Pomelo";
import { useTabs } from "./tabs";

const dateText = () => {
  const d = new Date();
  return `${d.getMonth() + 1} 月 ${d.getDate()} 日 · 周${"日一二三四五六"[d.getDay()]}`;
};

/** One open page in「桌上开着」: slaps on when it's newly opened, peels off when closed. */
function OpenTab(props: { path: string; on: boolean; fresh: boolean; onSelect: () => void; onClose: () => void }) {
  const [leaving, setLeaving] = useState(false);
  const def = moduleById(splitPath(props.path).module);
  const name = pageName(props.path);
  const close = () => {
    if (reducedMotion()) return props.onClose();
    setLeaving(true);
    setTimeout(props.onClose, 300);
  };
  return (
    <div
      role="tab"
      aria-selected={props.on}
      aria-label={name}
      tabIndex={0}
      onClick={props.onSelect}
      onKeyDown={(e) => e.key === "Enter" && props.onSelect()}
      onAuxClick={(e) => e.button === 1 && close()}
      className={`ob pm-press pm-open-tab pm-tip-host ${props.on ? "on" : ""} ${props.fresh ? "pm-slap-in is-in" : ""} ${leaving ? "is-leaving" : ""}`}
    >
      <ModuleIcon def={def} size={30} />
      <span className="ul" data-hand="rule" data-seed="700" data-sw="3" />
      <button
        type="button"
        className="ux"
        aria-label={`关掉「${name}」`}
        onClick={(e) => {
          e.stopPropagation();
          close();
        }}
      >
        <HandIcon name="x" size={11} seed={3} />
      </button>
      <Tip>{name}</Tip>
    </div>
  );
}

export function TopBar(props: {
  isDev?: boolean;
  bloomOpen: boolean;
  onPomelo: (el: HTMLElement) => void;
  onSearch?: () => void;
  onChat?: () => void;
  chatOpen: boolean;
  unseen: boolean;
  face?: Face | null;
}) {
  const tabs = useTabs();
  const [menu, setMenu] = useState<MenuAt | null>(null);
  // the pages open when the window opened are already on the desk: only later ones slap on
  const seen = useRef<Set<number> | null>(null);
  if (seen.current == null) seen.current = new Set(tabs.tabs.map((t) => t.id));
  useEffect(() => {
    const t = setTimeout(() => tabs.tabs.forEach((x) => seen.current!.add(x.id)), 600);
    return () => clearTimeout(t);
  }, [tabs.tabs]);
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
        return { label: splitPath(s.path).module ? pageName(s.path) : "桌面", icon: <Icon size={14} style={{ color: m.color }} />, onSelect: () => tabs.jump(s.index) };
      }),
    });
  };
  return (
    <header data-tauri-drag-region className="topbar">
      <PomeloButton open={props.bloomOpen} onClick={props.onPomelo} face={props.face} />
      <button type="button" aria-label="后退" disabled={!tabs.canBack} onClick={tabs.back} onContextMenu={trail(-1)} className="navb pm-press pm-tip-host">
        <HandIcon name="back" seed={11} />
        <Tip>后退</Tip>
      </button>
      <button type="button" aria-label="前进" disabled={!tabs.canForward} onClick={tabs.forward} onContextMenu={trail(1)} className="navb pm-press pm-tip-host">
        <HandIcon name="fwd" seed={12} />
        <Tip>前进</Tip>
      </button>
      {!tabs.home && (
        <button type="button" onClick={tabs.goHome} className="tohome pm-press">
          <Tape kind="mint" rot={-2} />
          <span className="tl">
            <HandIcon name="back2" size={17} seed={13} />
            回桌面
          </span>
        </button>
      )}
      <div data-tauri-drag-region className="min-w-0 flex-1 self-stretch" />
      {tabs.tabs.length > 0 && (
        <span data-tauri-drag-region className="openlbl">
          桌上开着
        </span>
      )}
      <div role="tablist" aria-label="桌上开着的页" className="opens">
        {tabs.tabs.map((t, i) => (
          <OpenTab key={t.id} path={t.path} on={i === tabs.active} fresh={!seen.current!.has(t.id)} onSelect={() => tabs.select(i)} onClose={() => tabs.close(tabs.tabs.findIndex((x) => x.id === t.id))} />
        ))}
      </div>
      <button type="button" aria-label="新开一页（贴纸簿）" onClick={tabs.newTab} className="ib pm-press pm-tip-host">
        <HandIcon name="plus" seed={14} />
        <Tip>新开一页</Tip>
      </button>
      {props.isDev && <span className="devchip">开发版</span>}
      <span className="datechip max-[1099px]:hidden" data-hand="frame" data-r="4" data-fill="card" data-tilt="1.5" data-sw="1.4" data-amp="0.5" data-seed="701">
        <span>{dateText()}</span>
      </span>
      {props.onSearch && (
        <button type="button" aria-label="找东西（⌘K）" onClick={props.onSearch} className="ib pm-press pm-tip-host">
          <HandIcon name="search" size={21} seed={15} />
          <Tip>找东西</Tip>
        </button>
      )}
      <button type="button" aria-label="设置" onClick={() => tabs.open("/settings")} className="ib pm-press pm-tip-host">
        <HandIcon name="gear" size={21} seed={16} />
        <Tip>设置</Tip>
      </button>
      <AskButton open={props.chatOpen} onClick={props.onChat} unseen={props.unseen} />
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
    </header>
  );
}

/** 柚子's button (⌘J): 柚子 peeking out with a speech bubble, synced from visual-atelier (the bird until the sync). */
function AskButton(props: { open: boolean; onClick?: () => void; unseen: boolean }) {
  const ai = markSvg("ai-button");
  const label = props.open ? "收起柚子" : "问柚子";
  const dot = props.unseen && <span aria-label="有新内容" className="absolute top-0.5 right-0.5 size-2 rounded-full border border-[var(--color-top)] bg-primary" />;
  return (
    <button type="button" aria-label={`${label}（⌘J）`} aria-pressed={props.open} disabled={!props.onClick} onClick={props.onClick} className={`ask pm-press pm-tip-host ${props.open ? "on" : ""}`}>
      {ai ? <Mark svg={ai} size={36} /> : <Bird size={16} strokeWidth={1.75} />}
      {dot}
      <Tip>{label}</Tip>
    </button>
  );
}
