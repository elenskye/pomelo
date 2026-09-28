import { useEffect, useMemo, useRef, useState } from "react";
import { Plus } from "lucide-react";
import { moduleById } from "@/app/modules";
import { useConfirm } from "@/shared/feedback";
import { STATUS_ORDER, type StatusCode } from "@/shared/status";
import { Button, EmptyState, PageHeader, SearchInput, Segmented, Select } from "@/shared/ui";
import { useItems } from "./api";
import { CollectionPanel } from "./CollectionPanel";
import { CollectionTable } from "./CollectionTable";
import type { CollectionApi, CollectionConfig, Item } from "./types";

type Filter = "all" | StatusCode;
type Sort = "recent" | "title" | "rating" | "finished";
type Selection = number | "new" | null;

const collator = new Intl.Collator("zh-CN");

function arrange(items: Item[], query: string, filter: Filter, kind: string, sort: Sort): Item[] {
  const q = query.trim().toLowerCase();
  const xs = items.filter(
    (it) =>
      (filter === "all" || it.status === filter) &&
      (kind === "all" || it.kind === kind) &&
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
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [kind, setKind] = useState("all");
  const [sort, setSort] = useState<Sort>("recent");
  const [selection, setSelection] = useState<Selection>(null);
  const dirty = useRef(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const [confirm, confirmDialog] = useConfirm();
  const labels = config.statusLabels;

  const counts = useMemo(() => {
    const c: Record<StatusCode, number> = { unmarked: 0, todo: 0, doing: 0, done: 0 };
    items.forEach((it) => c[it.status]++);
    return c;
  }, [items]);
  const visible = useMemo(() => arrange(items, query, filter, kind, sort), [items, query, filter, kind, sort]);
  const creators = useMemo(() => distinct(items.map((it) => it.creator)), [items]);
  const countries = useMemo(() => distinct(items.map((it) => it.country)), [items]);
  const selected = typeof selection === "number" ? (items.find((it) => it.id === selection) ?? null) : null;

  const requestSelect = async (next: Selection) => {
    if (next === selection) return;
    if (dirty.current) {
      const ok = await confirm({ title: "放弃没有保存的修改？", confirmLabel: "放弃修改", danger: true });
      if (!ok) return;
    }
    dirty.current = false;
    setSelection(next);
  };
  const done = () => {
    dirty.current = false;
    setSelection(null);
  };

  // ⌘N new · ⌘F search · Esc close panel (native dialogs handle their own Esc)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector("dialog[open]")) return;
      if (e.metaKey && e.key === "n") {
        e.preventDefault();
        void requestSelect("new");
      } else if (e.metaKey && e.key === "f") {
        e.preventDefault();
        searchRef.current?.focus();
      } else if (e.key === "Escape" && selection !== null) {
        void requestSelect(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
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
    <Button variant="primary" onClick={() => void requestSelect("new")}>
      <Plus size={14} /> {config.newLabel}
    </Button>
  );

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
          <Select ariaLabel="按类型筛选" value={kind} onChange={setKind}>
            <option value="all">全部类型</option>
            {config.kinds.map((k) => (
              <option key={k.value} value={k.value}>
                {k.label}
              </option>
            ))}
          </Select>
          <Select ariaLabel="排序" value={sort} onChange={(v) => setSort(v as Sort)}>
            <option value="recent">最近添加</option>
            <option value="title">{config.titleLabel}</option>
            <option value="rating">评分</option>
            <option value="finished">完成时间</option>
          </Select>
          {newButton}
        </div>
      )}

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-8 pb-8">
        {isPending ? null : items.length === 0 ? (
          <EmptyState title={config.emptyTitle} hint={config.emptyHint} action={newButton} />
        ) : visible.length === 0 ? (
          <EmptyState title={`没有找到符合条件的${config.itemName}`} />
        ) : (
          <CollectionTable
            config={config}
            api={api}
            items={visible}
            selectedId={typeof selection === "number" ? selection : null}
            onSelect={(id) => void requestSelect(id)}
          />
        )}
      </div>

      {selection !== null && (selection === "new" || selected) && (
        <CollectionPanel
          key={selection}
          config={config}
          api={api}
          item={selected}
          creators={creators}
          countries={countries}
          onClose={() => void requestSelect(null)}
          onDone={done}
          onDirtyChange={(d) => {
            dirty.current = d;
          }}
        />
      )}
      {confirmDialog}
    </div>
  );
}
