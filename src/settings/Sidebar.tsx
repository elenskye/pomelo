// 设置 → 侧栏: show / hide each module (hidden ones leave the sidebar and 全局搜索; data stays).
import { useHiddenModules } from "@/app/hidden";
import { MODULES } from "@/app/modules";
import { Switch } from "@/shared/ui";
import { Section } from "./layout";

export function SidebarSection() {
  const [hidden, setHidden] = useHiddenModules();
  return (
    <Section title="侧栏" help="关掉的模块不在侧栏里出现，全局搜索也不搜它；数据都还在，再打开就回来了。顺序在侧栏里按住拖动调整。">
      <div className="grid grid-cols-3 gap-x-6 px-4 py-1">
        {MODULES.map((m) => {
          const Icon = m.icon;
          const shown = !hidden.includes(m.id);
          return (
            <div key={m.id} className="flex items-center gap-2.5 py-2">
              <Icon size={16} strokeWidth={1.5} style={{ color: m.color }} className={`shrink-0 ${shown ? "" : "opacity-40"}`} />
              <span className={`flex-1 ${shown ? "" : "text-muted-foreground"}`}>{m.label}</span>
              <Switch label={`在侧栏显示「${m.label}」`} checked={shown} onChange={(v) => setHidden(v ? hidden.filter((h) => h !== m.id) : [...hidden, m.id])} />
            </div>
          );
        })}
      </div>
    </Section>
  );
}
