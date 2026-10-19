// 来日方长: the pinned one as a big card, then one list in days.rs order (过期的证件, 还有 N 天, past
// one-offs last); tabs 全部 · 纪念日 · 生日 · 证件. ⌘N adds one of the tab's kind.
import { useState } from "react";
import { Pin, Plus } from "lucide-react";
import { moduleById } from "@/app/modules";
import { usePageContext } from "@/app/pageContext";
import { slashDate, weekday } from "@/shared/dates";
import { useOpenRequest } from "@/shared/open";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { Button, EmptyState, PageHeader, Segmented } from "@/shared/ui";
import { KIND_LABEL, REPEAT_LABEL, countText, nthText, useDays, type Day, type Kind } from "./api";
import { DayPanel } from "./DayPanel";

type Tab = "all" | Kind;
const TABS: { value: Tab; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "day", label: KIND_LABEL.day },
  { value: "birthday", label: KIND_LABEL.birthday },
  { value: "document", label: KIND_LABEL.document },
];
const EMPTY: Record<Tab, string> = { all: "来日方长，日子还空着", day: "还没有纪念日", birthday: "还没有记生日", document: "还没有证件" };

/** 今天 柚红, 过期的证件 red, 快到的 yellow. */
function tone(d: Day) {
  if (d.days === 0) return "text-primary";
  if (d.days < 0) return d.kind === "document" ? "text-destructive" : "";
  return d.soon ? "text-[var(--status-doing-fg)]" : "";
}

function Count(props: { d: Day; big?: boolean }) {
  const [before, n, after] = countText(props.d);
  return (
    <span className={`shrink-0 whitespace-nowrap tabular-nums ${tone(props.d)}`}>
      {before}
      {n !== null && <b className={props.big ? "mx-1.5 text-[40px] leading-none" : "mx-1 text-base"}>{n}</b>}
      {after}
    </span>
  );
}

function PinnedCard(props: { d: Day; selected: boolean; onOpen: () => void }) {
  const d = props.d;
  const bits = [`${slashDate(d.next)} ${weekday(d.next)}`, d.kind === "day" && d.repeat !== "none" ? REPEAT_LABEL[d.repeat] : null, nthText(d)];
  return (
    <button
      type="button"
      onClick={props.onOpen}
      className={`flex w-full items-center justify-between gap-6 rounded-card border px-6 py-5 text-left transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${props.selected ? "bg-accent" : "bg-card hover:bg-muted/40"}`}
    >
      <span className="min-w-0 space-y-1">
        <span className="flex items-center gap-1 text-xs text-muted-foreground">
          <Pin size={12} className="text-primary" />
          {KIND_LABEL[d.kind]}
        </span>
        <span className="block truncate text-lg font-bold">{d.name}</span>
        <span className="block truncate text-xs text-muted-foreground tabular-nums">{bits.filter(Boolean).join(" · ")}</span>
      </span>
      <Count d={d} big />
    </button>
  );
}

function DayRow(props: { d: Day; selected: boolean; onOpen: () => void }) {
  const d = props.d;
  return (
    <button
      type="button"
      onClick={props.onOpen}
      className={`flex w-full items-baseline gap-3 border-b px-4 py-2.5 text-left text-sm transition-colors duration-150 outline-none last:border-b-0 focus-visible:bg-muted/70 ${props.selected ? "bg-accent" : "hover:bg-muted/70"}`}
    >
      <span className="min-w-0 truncate">{d.name}</span>
      <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground tabular-nums">{[slashDate(d.next), nthText(d)].filter(Boolean).join(" · ")}</span>
      <Count d={d} />
    </button>
  );
}

export default function DaysPage() {
  const m = moduleById("days");
  const [tab, setTab] = useState<Tab>("all");
  const { data: days = [] } = useDays();
  const panel = usePanelSelection<number | "new">();
  useOpenRequest((h) => {
    setTab("all");
    void panel.request(h.id);
  });
  usePageKeys({ onNew: () => void panel.request("new"), onEscape: panel.selection !== null ? () => void panel.request(null) : undefined });
  const editing = typeof panel.selection === "number" ? days.find((d) => d.id === panel.selection) : undefined;
  const label = TABS.find((t) => t.value === tab)!.label;
  usePageContext({ module: "days", page: label, record: editing ? { kind: "day", id: editing.id, label: `日子：${editing.name}` } : null });

  const shown = days.filter((d) => tab === "all" || d.kind === tab);
  const pinned = shown.find((d) => d.pinned);
  const rest = shown.filter((d) => !d.pinned);
  const soon = days.filter((d) => d.soon).length;
  const count = (t: Tab) => (t === "all" ? days.length : days.filter((d) => d.kind === t).length);

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader
        icon={m.icon}
        color={m.color}
        title={m.label}
        right={
          <p className="pointer-events-none text-sm text-muted-foreground">
            共 <b className="text-base text-foreground">{days.length}</b> 个
            {soon > 0 && (
              <>
                {" "}
                · 快到了 <b className="text-base text-[var(--status-doing-fg)]">{soon}</b>
              </>
            )}
          </p>
        }
      />
      <div className="flex min-h-0 flex-1 flex-col px-8 pb-6">
        <div className="flex shrink-0 items-center justify-between gap-4 pb-3">
          <Segmented
            ariaLabel="类型"
            value={tab}
            onChange={setTab}
            options={TABS.map((t) => ({
              value: t.value,
              label: (
                <>
                  {t.label} <span className="text-xs text-muted-foreground">{count(t.value)}</span>
                </>
              ),
            }))}
          />
          <Button variant="primary" onClick={() => void panel.request("new")}>
            <Plus size={14} /> 新日子
          </Button>
        </div>
        {shown.length === 0 ? (
          <EmptyState title={EMPTY[tab]} />
        ) : (
          <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
            <div className="max-w-3xl space-y-4">
              {pinned && <PinnedCard d={pinned} selected={editing?.id === pinned.id} onOpen={() => void panel.request(pinned.id)} />}
              {rest.length > 0 && (
                <div className="overflow-hidden rounded-card border bg-card">
                  {rest.map((d) => (
                    <DayRow key={d.id} d={d} selected={editing?.id === d.id} onOpen={() => void panel.request(d.id)} />
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
      {(panel.selection === "new" || editing) && (
        <DayPanel
          key={String(panel.selection)}
          day={editing ?? null}
          newKind={tab === "all" ? "day" : tab}
          onClose={() => void panel.request(null)}
          onDone={panel.done}
          onDirtyChange={panel.setDirty}
        />
      )}
      {panel.confirmDialog}
    </div>
  );
}
