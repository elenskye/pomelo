// 钱来钱往: left nav (收支 · 资产 · 报表) + the chosen view. Each view owns its panel and dialogs. 导入账单 (Importer,
// `/finance/import`) lies over 收支, which stays underneath as it was.
import { useState } from "react";
import { moduleById } from "@/app/modules";
import { useSubNav, useSubPage } from "@/app/subpage";
import { useOpenRequest, type Hit } from "@/shared/open";
import { PageHeader } from "@/shared/ui";
import { AssetsView } from "./AssetsView";
import { EntriesView } from "./EntriesView";
import { ImportPanel } from "./ImportPanel";
import { ReportsView } from "./ReportsView";

type View = "entries" | "assets" | "reports" | "import";

export default function FinancePage() {
  const m = moduleById("finance");
  const [view, setView] = useSubPage<View>("finance");
  // its pages are the folder tabs on the paper's top edge (the frame)
  useSubNav("finance", (p) => {
    setFocus(null);
    setView(p as View);
  });
  // 全局搜索: switch to the view that has it; the view opens the record
  const [focus, setFocus] = useState<Hit | null>(null);
  const [posted, setPosted] = useState<string | null>(null);
  useOpenRequest((h) => {
    setView(h.kind === "account" || h.kind === "assets" ? "assets" : "entries");
    setFocus(h);
  });
  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader icon={m.icon} color={m.color} title={m.label} />
      <div className="flex min-h-0 flex-1 gap-6 px-8 pb-6">
        <section className="flex min-w-0 flex-1 flex-col">
          {(view === "entries" || view === "import") && <EntriesView focus={focus} showMonth={posted} onImport={() => setView("import")} />}
          {view === "assets" && <AssetsView focus={focus} />}
          {view === "reports" && <ReportsView />}
        </section>
      </div>
      {view === "import" && <ImportPanel onClose={() => setView("entries")} onPosted={(day) => setPosted(day.slice(0, 7))} />}
    </div>
  );
}
