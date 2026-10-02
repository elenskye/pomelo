import { useMemo, useRef, useState } from "react";
import { Plus } from "lucide-react";
import { moduleById } from "@/app/modules";
import { Dropdown } from "@/shared/Dropdown";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { KindManager } from "@/shared/kinds/KindManager";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { STATUS_ORDER, type StatusCode } from "@/shared/status";
import { Button, EmptyState, PageHeader, SearchInput, Segmented } from "@/shared/ui";
import { useItems } from "./api";
import { CollectionPanel } from "./CollectionPanel";
import { CollectionTable } from "./CollectionTable";
import type { CollectionApi, CollectionConfig, Item } from "./types";

type Filter = "all" | StatusCode;
type KindFilter = "all" | "none" | number;
type Sort = "recent" | "title" | "rating" | "finished";

const collator = new Intl.Collator("zh-CN");

function arrange(items: Item[], query: string, filter: Filter, kind: KindFilter, sort: Sort): Item[] {
  const q = query.trim().toLowerCase();
  const xs = items.filter(
    (it) =>
      (filter === "all" || it.status === filter) &&
      (kind === "all" || (kind === "none" ? it.kindId === null : it.kindId === kind)) &&
      (!q || [it.title, it.creator, it.country, it.note].some((v) => v?.toLowerCase().includes(q))),
  );
  const byRecent = (a: Item, b: Item) => b.id - a.id;
  const cmp: Record<Sort, (a: Item, b: Item) => number> = {
    recent: byRecent,
    title: (a, b) => collator.compare(a.title, b.title),
    rating: (a, b) => (b.rating ?? 0) - (a.rating ?? 0) || byRecent(a, b),
    finished: (a, b) => (b.finishedMonth ?? "").localeCompare(a.finishedMonth ?? "") || byRecent(a, b),
  };
  return xs.sort(cmp[sort]);
}

const distinct = (xs: (string | null)[]) =>
  [...new Set(xs.filter((x): x is string => !!x))].sort((a, b) => collator.compare(a, b));

/** Full page for a collection module: stats, search, status/kind filters, sort, table, editor panel. */
export function CollectionPage({ config, api }: { config: CollectionConfig; api: CollectionApi }) {
  const m = moduleById(config.moduleId);
  const { data: items = [], isPending } = useItems(api);
  const { data: kinds = [] } = useKinds(config.moduleId);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [kind, setKind] = useState<KindFilter>("all");
  const [sort, setSort] = useState<Sort>("recent");
  const [managingKinds, setManagingKinds] = useState(false);
  const panel = usePanelSelection<number | "new">();
  const searchRef = useRef<HTMLInputElement>(null);
  const labels = config.statusLabels;

  const counts = useMemo(() => {
    const c: Record<StatusCode, number> = { unmarked: 0, todo: 0, doing: 0, done: 0 };
    items.forEach((it) => c[it.status]++);
    return c;
  }, [items]);
  const uncategorized = useMemo(() => items.filter((it) => it.kindId === null).length, [items]);
  // a deleted kind (or an emptied 未分类) can't stay selected as a filter
  const kindFilter: KindFilter =
    (typeof kind === "number" && !kinds.some((k) => k.id === kind)) || (kind === "none" && uncategorized === 0) ? "all" : kind;
  const visible = useMemo(
    () => arrange(items, query, filter, kindFilter, sort),
    [items, query, filter, kindFilter, sort],
  );
  const creators = useMemo(() => distinct(items.map((it) => it.creator)), [items]);
  const countries = useMemo(() => distinct(items.map((it) => it.country)), [items]);
  const sel = panel.selection;
  const selected = typeof sel === "number" ? (items.find((it) => it.id === sel) ?? null) : null;

  usePageKeys({
    onNew: () => void panel.request("new"),
    onFind: () => searchRef.current?.focus(),
    onEscape: sel !== null ? () => void panel.request(null) : undefined,
  });

  const stats = (
    <p className="pointer-events-none text-sm text-muted-foreground">
      共 <b className="text-base text-foreground">{items.length}</b>
      {(["done", "doing", "todo", "unmarked"] as const)
        .filter((s) => s !== "unmarked" || counts.unmarked > 0)
        .map((s) => (
          <span key={s}>
            {" · "}
            {labels[s]} <b className="text-base text-foreground">{counts[s]}</b>
          </span>
        ))}
    </p>
  );

  const newButton = (
    <Button variant="primary" onClick={() => void panel.request("new")}>
      <Plus size={14} /> {config.newLabel}
    </Button>
  );
  const manageKinds = { label: "管理类型…", onSelect: () => setManagingKinds(true) };

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader icon={m.icon} color={m.color} title={m.label} right={items.length > 0 && stats} />

      {items.length > 0 && (
        <div className="flex shrink-0 flex-wrap items-center gap-3 px-8 pb-4">
          <div className="w-56">
            <SearchInput
              ref={searchRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={`搜索${config.titleLabel}、${config.creatorLabel}、国家、备注`}
            />
          </div>
          <Segmented
            ariaLabel="按状态筛选"
            value={filter}
            onChange={setFilter}
            options={[
              { value: "all" as Filter, label: `全部 ${items.length}` },
              ...STATUS_ORDER.map((s) => ({ value: s as Filter, label: `${labels[s]} ${counts[s]}` })),
            ]}
          />
          <div className="flex-1" />
          <Dropdown<KindFilter>
            ariaLabel="按类型筛选"
            align="end"
            value={kindFilter}
            onChange={setKind}
            options={[
              { value: "all", label: "全部类型" },
              ...kinds.map((k) => ({ value: k.id as KindFilter, label: k.name })),
              ...(uncategorized > 0 ? [{ value: "none" as KindFilter, label: UNCATEGORIZED }] : []),
            ]}
            actions={[manageKinds]}
          />
          <Dropdown<Sort>
            ariaLabel="排序"
            align="end"
            value={sort}
            onChange={setSort}
            options={[
              { value: "recent", label: "最近添加" },
              { value: "title", label: config.titleLabel },
              { value: "rating", label: "评分" },
              { value: "finished", label: "完成时间" },
            ]}
          />
          {newButton}
        </div>
      )}

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-8 pb-8">
        {isPending ? null : items.length === 0 ? (
          <EmptyState title={config.emptyTitle} action={newButton} />
        ) : visible.length === 0 ? (
          <EmptyState title={`没有找到符合条件的${config.itemName}`} />
        ) : (
          <CollectionTable
            config={config}
            api={api}
            items={visible}
            selectedId={typeof sel === "number" ? sel : null}
            onSelect={(id) => void panel.request(id)}
          />
        )}
      </div>

      {sel !== null && (sel === "new" || selected) && (
        <CollectionPanel
          key={sel}
          config={config}
          api={api}
          item={selected}
          creators={creators}
          countries={countries}
          onClose={() => void panel.request(null)}
          onDone={panel.done}
          onDirtyChange={panel.setDirty}
          onManageKinds={() => setManagingKinds(true)}
        />
      )}
      {managingKinds && (
        <KindManager
          module={config.moduleId}
          moduleLabel={m.label}
          unit={config.unit}
          itemsKey={api.key}
          onClose={() => setManagingKinds(false)}
        />
      )}
      {panel.confirmDialog}
    </div>
  );
}
