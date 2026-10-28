// 钱来钱往: left nav (收支 · 资产 · 报表) + the chosen view. Each view owns its panel and dialogs.
import { useState } from "react";
import { ChartPie, Landmark, ReceiptText, type LucideIcon } from "lucide-react";
import { moduleById } from "@/app/modules";
import { useSubPage } from "@/app/subpage";
import { useOpenRequest, type Hit } from "@/shared/open";
import { PageHeader } from "@/shared/ui";
import { AssetsView } from "./AssetsView";
import { EntriesView } from "./EntriesView";
import { ReportsView } from "./ReportsView";

type View = "entries" | "assets" | "reports";
const VIEWS: { id: View; label: string; icon: LucideIcon }[] = [
  { id: "entries", label: "收支", icon: ReceiptText },
  { id: "assets", label: "资产", icon: Landmark },
  { id: "reports", label: "报表", icon: ChartPie },
];

export default function FinancePage() {
  const m = moduleById("finance");
  const [view, setView] = useSubPage<View>("finance");
  // 全局搜索: switch to the view that has it; the view opens the record
  const [focus, setFocus] = useState<Hit | null>(null);
  useOpenRequest((h) => {
    setView(h.kind === "account" || h.kind === "assets" ? "assets" : "entries");
    setFocus(h);
  });
  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader icon={m.icon} color={m.color} title={m.label} />
      <div className="flex min-h-0 flex-1 gap-6 px-8 pb-6">
        <nav aria-label={m.label} className="flex w-40 shrink-0 flex-col gap-0.5">
          {VIEWS.map((v) => {
            const Icon = v.icon;
            const active = v.id === view;
            return (
              <button
                key={v.id}
                type="button"
                aria-current={active}
                onClick={() => {
                  setFocus(null);
                  setView(v.id);
                }}
                className={`flex items-center gap-2 rounded-control px-2.5 py-1.5 text-left outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring ${
                  active ? "bg-accent" : "hover:bg-muted/70"
                }`}
              >
                <Icon size={15} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
                {v.label}
              </button>
            );
          })}
        </nav>
        <section className="flex min-w-0 flex-1 flex-col">
          {view === "entries" && <EntriesView focus={focus} />}
          {view === "assets" && <AssetsView focus={focus} />}
          {view === "reports" && <ReportsView />}
        </section>
      </div>
    </div>
  );
}
