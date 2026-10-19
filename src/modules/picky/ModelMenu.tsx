// The model button (page header): local models + cloud models in one menu; choosing a local model
// switches to it (starting it if needed). Shares its state with the menu bar (Rust keeps both).
import { ChevronDown, Cloud } from "lucide-react";
import { Dropdown, type DropdownAction, type DropdownOption } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { Button } from "@/shared/ui";
import { pickyApi, selectedLabel, useRefreshStatus, type Dot, type Issue, type IssueAction, type PickyStatus } from "./api";

const DOT_COLOR: Record<Dot, string> = {
  ready: "var(--status-done-fg)",
  loading: "var(--status-doing-fg)",
  error: "var(--destructive)",
  off: "var(--muted-foreground)",
};

export function StatusDot(props: { dot: Dot | "cloud" }) {
  if (props.dot === "cloud") return <Cloud size={14} strokeWidth={1.5} className="shrink-0 text-muted-foreground" />;
  const hollow = props.dot === "off";
  return (
    <span
      className={`inline-block size-2 shrink-0 rounded-full ${props.dot === "loading" ? "animate-pulse" : ""}`}
      style={hollow ? { boxShadow: `inset 0 0 0 1.5px ${DOT_COLOR.off}` } : { background: DOT_COLOR[props.dot] }}
    />
  );
}

export function ModelMenu(props: { status: PickyStatus }) {
  const s = props.status;
  const toast = useToast();
  const refresh = useRefreshStatus();
  const label = selectedLabel(s);
  const value = s.selection === "local" ? (s.local.active ? `local:${s.local.active.key}` : "") : s.selection;
  const options: DropdownOption<string>[] = [
    ...s.local.models.map((m) => ({ value: `local:${m.key}`, label: m.name, group: "本地模型" })),
    ...s.cloud.map((m) => ({ value: `cloud:${m.id}`, label: m.name, group: "云端模型" })),
  ];
  // replies being written keep their model: switching is fine, except the local model while it answers
  const localBusy = s.active.some((a) => a.model.startsWith("local:"));
  const actions: DropdownAction[] =
    s.local.active && !s.local.working && !localBusy
      ? [{ label: "停止本地模型", onSelect: () => void pickyApi.stopModel().then(refresh, (e) => toast(errorText(e), "error")) }]
      : [];
  const trigger = (
    <span className="flex items-center gap-2">
      <StatusDot dot={label.dot} />
      <span className="max-w-56 truncate">{label.title}</span>
      {label.missingKey && <span className="text-xs text-destructive">缺 Key</span>}
    </span>
  );
  if (s.local.working)
    return <span className="inline-flex items-center gap-1.5 rounded-control border bg-card py-1.5 pr-3 pl-3 text-sm text-muted-foreground">{trigger}</span>;
  return (
    <Dropdown
      ariaLabel="选择模型"
      align="end"
      value={value}
      options={options}
      actions={actions}
      onChange={(choice) => void pickyApi.select(choice).then(refresh, (e) => toast(errorText(e), "error"))}
      renderTrigger={(_, open) => (
        <>
          {trigger}
          <ChevronDown size={14} className={`text-muted-foreground transition-transform duration-150 ${open ? "rotate-180" : ""}`} />
        </>
      )}
    />
  );
}

const ACTION_LABEL: Record<IssueAction["kind"], string> = {
  retryStart: "重试",
  retryStop: "重试",
  refresh: "重试",
  openServerLog: "打开服务日志",
  openCatalog: "打开模型清单",
  openServiceLog: "打开服务日志",
};

/** A model-service problem: what happened, why, what next (raw output on hover). */
export function IssueNotice(props: { issue: Issue; compact?: boolean }) {
  const toast = useToast();
  const refresh = useRefreshStatus();
  const { issue } = props;
  const run = (a: IssueAction) => void pickyApi.issueAction(a).then(refresh, (e) => toast(errorText(e), "error"));
  return (
    <div className={`rounded-card border bg-card text-sm ${props.compact ? "px-3 py-2" : "px-4 py-3"}`} title={issue.details ?? undefined}>
      <p className="font-bold text-destructive">{issue.title}</p>
      <p className="mt-0.5 text-muted-foreground">{issue.message}</p>
      <div className="mt-2 flex flex-wrap gap-2">
        {issue.actions.map((a, i) => (
          <Button key={i} className="py-1" onClick={() => run(a)}>
            {ACTION_LABEL[a.kind]}
          </Button>
        ))}
        {issue.kind !== "catalog" && (
          <Button variant="ghost" className="py-1" onClick={() => void pickyApi.issueDismiss(issue.kind).then(refresh)}>
            关闭
          </Button>
        )}
      </div>
    </div>
  );
}
