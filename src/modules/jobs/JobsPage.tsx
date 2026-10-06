// 投石问路: 投递 (table + 该跟进) and 统计, switched in the header. ⌘N adds an application.
import { useState } from "react";
import { Plus } from "lucide-react";
import { moduleById } from "@/app/modules";
import { KindManager } from "@/shared/kinds/KindManager";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { Button, PageHeader, Segmented } from "@/shared/ui";
import { CHANNELS, JOBS_KEY, useJobs } from "./api";
import { JobPanel } from "./JobPanel";
import { StatsView } from "./StatsView";
import { StatusManager } from "./StatusManager";
import { TableView } from "./TableView";

type View = "table" | "stats";

export default function JobsPage() {
  const m = moduleById("jobs");
  const [view, setView] = useState<View>("table");
  const { data: jobs = [] } = useJobs();
  const panel = usePanelSelection<number | "new">();
  const [managing, setManaging] = useState<"statuses" | "channels" | null>(null);
  usePageKeys({ onNew: () => void panel.request("new"), onEscape: panel.selection !== null ? () => void panel.request(null) : undefined });
  const editing = typeof panel.selection === "number" ? jobs.find((j) => j.id === panel.selection) : undefined;
  const following = jobs.filter((j) => j.followUp).length;

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader
        icon={m.icon}
        color={m.color}
        title={m.label}
        right={
          <p className="pointer-events-none text-sm text-muted-foreground">
            共 <b className="text-base text-foreground">{jobs.length}</b> 条
            {following > 0 && (
              <>
                {" "}
                · 该跟进 <b className="text-base text-[var(--status-doing-fg)]">{following}</b>
              </>
            )}
          </p>
        }
      />
      <div className="flex min-h-0 flex-1 flex-col px-8 pb-6">
        <div className="flex shrink-0 items-center justify-between gap-4 pb-3">
          <Segmented
            ariaLabel="投递还是统计"
            value={view}
            onChange={setView}
            options={[
              { value: "table" as View, label: "投递" },
              { value: "stats" as View, label: "统计" },
            ]}
          />
          <Button variant="primary" onClick={() => void panel.request("new")}>
            <Plus size={14} /> 新投递
          </Button>
        </div>
        {view === "table" ? <TableView jobs={jobs} selectedId={editing?.id ?? null} onOpen={(j) => void panel.request(j.id)} /> : <StatsView jobs={jobs} />}
      </div>
      {(panel.selection === "new" || editing) && (
        <JobPanel
          key={String(panel.selection)}
          job={editing ?? null}
          onClose={() => void panel.request(null)}
          onDone={panel.done}
          onDirtyChange={panel.setDirty}
          onManageStatuses={() => setManaging("statuses")}
          onManageChannels={() => setManaging("channels")}
        />
      )}
      {managing === "statuses" && <StatusManager onClose={() => setManaging(null)} />}
      {managing === "channels" && (
        <KindManager module={CHANNELS} moduleLabel={m.label} unit="条" noun="渠道" itemsKey={JOBS_KEY} onClose={() => setManaging(null)} />
      )}
      {panel.confirmDialog}
    </div>
  );
}
