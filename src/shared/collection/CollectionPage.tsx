import { useMemo, useRef, useState } from "react";
import { Plus } from "@/shared/icons";
import { moduleById } from "@/app/modules";
import { usePageContext } from "@/app/pageContext";
import { Dropdown } from "@/shared/Dropdown";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { KindManager } from "@/shared/kinds/KindManager";
import { useOpenRequest } from "@/shared/open";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { STATUS_ORDER, type StatusCode } from "@/shared/status";
import { Button, EmptyState, PageHeader, SearchInput, Segmented } from "@/shared/ui";
import { useItems } from "./api";
import { CollectionPanel } from "./CollectionPanel";
import { CollectionTable } from "./CollectionTable";
import { latestYear, type CollectionApi, type CollectionConfig, type Item } from "./types";

type Filter = "all" | StatusCode;
type KindFilter = "all" | "none" | number;
/** 玩物有志's 平台: like the 类型 filter. */
type PlatformFilter = KindFilter;
type Sort = "recent" | "title" | "rating" | "finished";

const collator = new Intl.Collator("zh-CN");

function arrange(items: Item[], query: string, filter: Filter, kind: KindFilter, platform: PlatformFilter, platformName: (id: number | null | undefined) => string | null, sort: Sort): Item[] {
  const q = query.trim().toLowerCase();
  const xs = items.filter(
    (it) =>
      (filter === "all" || it.status === filter) &&
      (kind === "all" || (kind === "none" ? it.kindId === null : it.kindId === kind)) &&
      (platform === "all" || (platform === "none" ? (it.platformId ?? null) === null : it.platformId === platform)) &&
      (!q || [it.title, it.creator, it.country, it.note, platformName(it.platformId)].some((v) => v?.toLowerCase().includes(q))),
  );
  const byRecent = (a: Item, b: Item) => b.id - a.id;
  const cmp: Record<Sort, (a: Item, b: Item) => number> = {
    recent: byRecent,
    title: (a, b) => collator.compare(a.title, b.title),
    rating: (a, b) => (b.rating ?? 0) - (a.rating ?? 0) || byRecent(a, b),
    finished: (a, b) => (latestYear(b) ?? "").localeCompare(latestYear(a) ?? "") || byRecent(a, b),
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
  const { data: platforms = [] } = useKinds(config.platforms?.module ?? "", !!config.platforms);
  const [query, setQuery] = useState("");
  const [platform, setPlatform] = useState<PlatformFilter>("all");
  const [managingPlatforms, setManagingPlatforms] = useState(false);
  const order = config.statuses ?? STATUS_ORDER;
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
  const noPlatform = useMemo(() => items.filter((it) => (it.platformId ?? null) === null).length, [items]);
  const platformFilter: PlatformFilter =
    (typeof platform === "number" && !platforms.some((k) => k.id === platform)) || (platform === "none" && noPlatform === 0) ? "all" : platform;
  const visible = useMemo(() => {
    const name = (id: number | null | undefined) => platforms.find((p) => p.id === id)?.name ?? null;
    return arrange(items, query, filter, kindFilter, config.platforms ? platformFilter : "all", name, sort);
  }, [items, query, filter, kindFilter, platformFilter, platforms, config.platforms, sort]);
  const creators = useMemo(() => distinct(items.map((it) => it.creator)), [items]);
  const countries = useMemo(() => distinct(items.map((it) => it.country)), [items]);
  const sel = panel.selection;
  const selected = typeof sel === "number" ? (items.find((it) => it.id === sel) ?? null) : null;

  usePageContext({
    module: config.moduleId,
    filter:
      [
        filter === "all" ? null : labels[filter],
        kindFilter === "all" ? null : kindFilter === "none" ? UNCATEGORIZED : (kinds.find((k) => k.id === kindFilter)?.name ?? null),
        !config.platforms || platformFilter === "all" ? null : platformFilter === "none" ? `没填${config.platforms.noun}` : (platforms.find((k) => k.id === platformFilter)?.name ?? null),
        query.trim() ? `搜「${query.trim()}」` : null,
      ]
        .filter(Boolean)
        .join(" · ") || null,
    record: selected ? { kind: config.record.kind, id: selected.id, label: `${config.record.label}：${selected.title}` } : null,
  });

  useOpenRequest((h) => void panel.request(h.id));
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
      <PageHeader title={m.label} right={items.length > 0 && stats} />

      {items.length > 0 && (
        <div className="flex shrink-0 flex-wrap items-center gap-3 px-8 pb-4">
          <div className="w-56">
            <SearchInput
              ref={searchRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={config.noCreator ? `搜索${config.titleLabel}、国家、备注` : `搜索${config.titleLabel}、${config.creatorLabel}、国家、备注`}
            />
          </div>
          <Segmented
            ariaLabel="按状态筛选"
            value={filter}
            onChange={setFilter}
            options={[
              { value: "all" as Filter, label: `全部 ${items.length}` },
              ...order.map((s) => ({ value: s as Filter, label: `${labels[s]} ${counts[s]}` })),
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
          {config.platforms && (
            <Dropdown<PlatformFilter>
              ariaLabel={`按${config.platforms.noun}筛选`}
              align="end"
              value={platformFilter}
              onChange={setPlatform}
              options={[
                { value: "all", label: `全部${config.platforms.noun}` },
                ...platforms.map((k) => ({ value: k.id as PlatformFilter, label: k.name })),
                ...(noPlatform > 0 ? [{ value: "none" as PlatformFilter, label: `没填${config.platforms.noun}` }] : []),
              ]}
              actions={[{ label: `管理${config.platforms.noun}…`, onSelect: () => setManagingPlatforms(true) }]}
            />
          )}
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

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-8 pt-1 pb-8">
        {isPending ? null : items.length === 0 ? (
          <EmptyState title={config.emptyTitle} art={config.emptyArt} action={newButton} />
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
          onManagePlatforms={() => setManagingPlatforms(true)}
        />
      )}
      {managingPlatforms && config.platforms && (
        <KindManager module={config.platforms.module} moduleLabel={m.label} unit={config.unit} itemsKey={api.key} noun={config.platforms.noun} onClose={() => setManagingPlatforms(false)} />
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
