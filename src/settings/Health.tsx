// 设置 → 体检 (rules in Rust health.rs): records worth a look, listed by what's wrong; each opens its record the way ⌘K does.
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { Stethoscope } from "@/shared/icons";
import { useTabs } from "@/app/tabs";
import { moduleById } from "@/app/modules";
import type { Hit } from "@/shared/open";
import { PageHeader } from "@/shared/ui";
import { Section } from "./layout";

type Group = { key: string; title: string; total: number; items: Hit[] };

export function useHealth() {
  return useQuery({ queryKey: ["health"], queryFn: () => invoke<Group[]>("health_check"), refetchOnMount: "always" });
}

export default function HealthPage() {
  const tabs = useTabs();
  const groups = useHealth().data;
  if (!groups) return null;
  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <PageHeader icon={Stethoscope} color="var(--muted-foreground)" title="体检" />
      <div className="px-8 py-4">
        {groups.map((g) => (
          <Section key={g.key} title={g.total ? `${g.title} · ${g.total}` : g.title}>
            {g.total === 0 ? (
              <p className="px-4 py-3 text-sm text-muted-foreground">没有</p>
            ) : (
              <ul className="divide-y">
                {g.items.map((h) => {
                  const m = moduleById(h.module);
                  const Icon = m.icon;
                  return (
                    <li key={`${h.module}:${h.kind}:${h.id}`}>
                      <button type="button" onClick={() => tabs.open(`/${h.module}`, { open: h })} className="flex w-full items-center gap-2.5 px-4 py-2 text-left text-sm outline-none first:rounded-t-card last:rounded-b-card hover:bg-muted/70 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset">
                        <Icon size={14} strokeWidth={1.75} className="shrink-0" style={{ color: m.color }} />
                        <span className="min-w-0 truncate">{h.title}</span>
                        {h.detail && <span className="ml-auto shrink-0 pl-4 text-xs text-muted-foreground">{h.detail}</span>}
                      </button>
                    </li>
                  );
                })}
                {g.total > g.items.length && <li className="px-4 py-2 text-xs text-muted-foreground">还有 {g.total - g.items.length} 条没列出</li>}
              </ul>
            )}
          </Section>
        ))}
      </div>
    </div>
  );
}
