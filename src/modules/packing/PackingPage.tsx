import { useMemo, useState, type ReactNode } from "react";
import { CheckCircle2, Circle, Pencil, Plus, Tags, X } from "lucide-react";
import { moduleById } from "@/app/modules";
import { useToast } from "@/shared/feedback";
import { useKinds, UNCATEGORIZED, type Kind } from "@/shared/kinds/api";
import { KindManager } from "@/shared/kinds/KindManager";
import { NameDialog } from "@/shared/NameDialog";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { DragHandle, useSortable } from "@/shared/sortable";
import { Button, EmptyState, IconButton, PageHeader, ProgressBar, Segmented } from "@/shared/ui";
import { formatDay } from "@/shared/dates";
import {
  PACKING_KEY,
  usePackingMutations,
  usePackItems,
  useTemplates,
  useTrips,
  type ListKind,
  type PackItem,
  type Template,
  type Trip,
} from "./api";
import { ItemPanel } from "./ItemPanel";
import { TripDialog } from "./TripDialog";

type Tab = "trips" | "templates";

/** One 类别's rows; draggable when `onReorder` is set. */
function SortableRows(props: { items: PackItem[]; row: (it: PackItem, grip: ReactNode) => ReactNode; onReorder?: (ids: number[]) => void }) {
  const sort = useSortable((ids) => props.onReorder?.(ids));
  return (
    <div ref={sort.listRef} className="overflow-hidden rounded-card border bg-card">
      {props.items.map((it) => (
        <div key={it.id} {...sort.item(it.id)} className="[&:last-child>div]:border-b-0">
          {props.row(it, props.onReorder && props.items.length > 1 ? <DragHandle {...sort.handle(it.id)} label="拖动物品排序" /> : null)}
        </div>
      ))}
    </div>
  );
}

/** Items grouped by 类别, in the user's 类别 order; 未分类 last. Dragging reorders within a 类别. */
function GroupedItems(props: {
  items: PackItem[];
  kinds: Kind[];
  row: (it: PackItem, grip: ReactNode) => ReactNode;
  groupNote?: (items: PackItem[]) => ReactNode;
  /** All items of the list in their new order (only when every item is shown). */
  onReorder?: (ids: number[]) => void;
}) {
  const groups = [
    ...props.kinds.map((k) => ({ key: String(k.id), name: k.name, items: props.items.filter((i) => i.kindId === k.id) })),
    { key: "none", name: UNCATEGORIZED, items: props.items.filter((i) => i.kindId === null || !props.kinds.some((k) => k.id === i.kindId)) },
  ].filter((g) => g.items.length > 0);
  return (
    <div className="space-y-4">
      {groups.map((g) => (
        <section key={g.key}>
          <h3 className="mb-1.5 flex items-baseline gap-2 px-1 text-sm text-muted-foreground">
            {g.name}
            <span className="text-xs">{props.groupNote?.(g.items) ?? g.items.length}</span>
          </h3>
          <SortableRows
            items={g.items}
            row={props.row}
            onReorder={
              props.onReorder &&
              ((ids) => {
                const inGroup = new Set(ids);
                const queue = [...ids];
                props.onReorder!(props.items.map((it) => (inGroup.has(it.id) ? queue.shift()! : it.id)));
              })
            }
          />
        </section>
      ))}
    </div>
  );
}

function RowActions(props: { name: string; onEdit: () => void; onDelete: () => void }) {
  return (
    <div className="flex opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100">
      <IconButton label={`编辑「${props.name}」`} onClick={props.onEdit}>
        <Pencil size={14} />
      </IconButton>
      <IconButton label={`删除「${props.name}」`} danger onClick={props.onDelete}>
        <X size={14} />
      </IconButton>
    </div>
  );
}

function SideList<T extends { id: number }>(props: {
  label: string;
  items: T[];
  currentId: number | null;
  onSelect: (id: number) => void;
  render: (it: T, grip: ReactNode) => ReactNode;
  newLabel: string;
  onNew: () => void;
  onReorder?: (ids: number[]) => void;
}) {
  const sort = useSortable((ids) => props.onReorder?.(ids));
  return (
    <nav aria-label={props.label} className="flex w-56 shrink-0 flex-col">
      <div ref={sort.listRef} className="min-h-0 flex-1 space-y-1 overflow-y-auto pr-1">
        {props.items.map((it) => (
          <button
            key={it.id}
            type="button"
            aria-current={it.id === props.currentId}
            onClick={() => props.onSelect(it.id)}
            {...sort.item(it.id)}
            className={`group w-full space-y-1.5 rounded-control px-3 py-2.5 text-left transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${
              it.id === props.currentId ? "bg-accent" : "bg-background hover:bg-muted/70"
            }`}
          >
            {props.render(it, props.onReorder && props.items.length > 1 ? <DragHandle {...sort.handle(it.id)} label={`拖动${props.label}排序`} /> : null)}
          </button>
        ))}
      </div>
      <Button variant="ghost" className="mt-2 justify-center text-muted-foreground" onClick={props.onNew}>
        <Plus size={14} /> {props.newLabel}
      </Button>
    </nav>
  );
}

export default function PackingPage() {
  const m = moduleById("packing");
  const [tab, setTab] = useState<Tab>("trips");
  const tripsQ = useTrips();
  const templatesQ = useTemplates();
  const trips = useMemo(() => tripsQ.data ?? [], [tripsQ.data]);
  const templates = useMemo(() => templatesQ.data ?? [], [templatesQ.data]);
  const { data: kinds = [] } = useKinds("packing");
  const [tripId, setTripId] = useState<number | null>(null);
  const [templateId, setTemplateId] = useState<number | null>(null);
  const trip = trips.find((t) => t.id === tripId) ?? trips[0] ?? null;
  const template = templates.find((t) => t.id === templateId) ?? templates[0] ?? null;
  const list: ListKind = tab === "trips" ? "trip" : "template";
  const parentId = tab === "trips" ? (trip?.id ?? null) : (template?.id ?? null);
  const itemsQ = usePackItems(list, parentId);
  const items = useMemo(() => itemsQ.data ?? [], [itemsQ.data]);
  const [onlyUnpacked, setOnlyUnpacked] = useState(false);
  const [tripDialog, setTripDialog] = useState<{ mode: "new"; templateId?: number | null } | { mode: "edit" } | null>(null);
  const [templateDialog, setTemplateDialog] = useState<"new" | "edit" | null>(null);
  const [managingKinds, setManagingKinds] = useState(false);
  const panel = usePanelSelection<number | "new">();
  const sel = panel.selection;
  const selectedItem = typeof sel === "number" ? (items.find((i) => i.id === sel) ?? null) : null;
  const mut = usePackingMutations();
  const toast = useToast();

  const switchTo = async (next: () => void) => {
    if (await panel.request(null)) next();
  };
  const removeItem = (it: PackItem) => {
    mut.quickDeleteItem.mutate({ list, id: it.id }, { onSuccess: () => toast(`已删除「${it.name}」`) });
  };

  usePageKeys({
    onNew: parentId !== null ? () => void panel.request("new") : undefined,
    onEscape: sel !== null ? () => void panel.request(null) : undefined,
  });

  const stats =
    trips.length + templates.length > 0 ? (
      <p className="pointer-events-none text-sm text-muted-foreground">
        <b className="text-base text-foreground">{trips.length}</b> 次出行 · <b className="text-base text-foreground">{templates.length}</b> 个模板
      </p>
    ) : null;

  const visible = tab === "trips" && onlyUnpacked ? items.filter((i) => !i.packed) : items;

  const tripRow = (it: PackItem, grip: ReactNode) => (
    <div
      key={it.id}
      role="checkbox"
      aria-checked={!!it.packed}
      tabIndex={0}
      onClick={() => mut.quickUpdateItem.mutate({ list: "trip", id: it.id, patch: { packed: !it.packed } })}
      onKeyDown={(e) => {
        if (e.key === " " || e.key === "Enter") {
          e.preventDefault();
          mut.quickUpdateItem.mutate({ list: "trip", id: it.id, patch: { packed: !it.packed } });
        }
      }}
      className="group flex min-h-10 cursor-pointer items-center gap-3 border-b bg-card py-1.5 pr-3 pl-1 outline-none transition-colors duration-150 hover:bg-muted/70 focus-visible:bg-muted/70"
    >
      <span className="flex w-3 shrink-0 justify-center">{grip}</span>
      {it.packed ? (
        <CheckCircle2 size={18} strokeWidth={1.75} className="shrink-0" style={{ color: "var(--status-done-fg)" }} />
      ) : (
        <Circle size={18} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
      )}
      <span className={`min-w-0 flex-1 ${it.packed ? "text-muted-foreground line-through decoration-1" : ""}`}>
        {it.name}
        {it.note && <span className="ml-2 text-xs text-muted-foreground no-underline">{it.note}</span>}
      </span>
      <div onClick={(e) => e.stopPropagation()}>
        <RowActions name={it.name} onEdit={() => void panel.request(it.id)} onDelete={() => removeItem(it)} />
      </div>
    </div>
  );

  const templateRow = (it: PackItem, grip: ReactNode) => (
    <div
      key={it.id}
      onClick={() => void panel.request(it.id)}
      className={`group flex min-h-10 cursor-pointer items-center gap-3 border-b py-1.5 pr-3 pl-1 transition-colors duration-150 ${
        it.id === sel ? "bg-accent" : "bg-card hover:bg-muted/70"
      }`}
    >
      <span className="flex w-3 shrink-0 justify-center">{grip}</span>
      <span className="size-1.5 shrink-0 rounded-full bg-muted-foreground/60" />
      <span className="min-w-0 flex-1">
        {it.name}
        {it.note && <span className="ml-2 text-xs text-muted-foreground">{it.note}</span>}
      </span>
      <div onClick={(e) => e.stopPropagation()}>
        <RowActions name={it.name} onEdit={() => void panel.request(it.id)} onDelete={() => removeItem(it)} />
      </div>
    </div>
  );

  const itemsArea = (emptyTitle: string) =>
    itemsQ.isPending ? null : items.length === 0 ? (
      <EmptyState
        title={emptyTitle}
        action={
          <Button variant="primary" onClick={() => void panel.request("new")}>
            <Plus size={14} /> 添加物品
          </Button>
        }
      />
    ) : visible.length === 0 ? (
      <EmptyState title="全部装好了，出发吧" />
    ) : (
      <GroupedItems
        items={visible}
        kinds={kinds}
        row={tab === "trips" ? tripRow : templateRow}
        groupNote={tab === "trips" ? (xs) => `${xs.filter((x) => x.packed).length}/${xs.length}` : undefined}
        onReorder={parentId !== null && visible.length === items.length ? (ids) => mut.reorderItems.mutate({ list, parentId, ids }) : undefined}
      />
    );

  const tripView = (t: Trip) => (
    <section className="flex min-w-0 flex-1 flex-col">
      <div className="flex shrink-0 items-start justify-between gap-4 pb-3">
        <div className="min-w-0">
          <div className="flex items-center gap-1">
            <h2 className="truncate text-lg font-bold">{t.name}</h2>
            <IconButton label="编辑出行" onClick={() => setTripDialog({ mode: "edit" })}>
              <Pencil size={14} />
            </IconButton>
          </div>
          <p className="text-sm text-muted-foreground">
            {[formatDay(t.departOn) && `${formatDay(t.departOn)}出发`, `已装好 ${t.packed} / ${t.total}`, t.templateName && `来自「${t.templateName}」`]
              .filter(Boolean)
              .join(" · ")}
          </p>
        </div>
        <Button variant="primary" onClick={() => void panel.request("new")}>
          <Plus size={14} /> 添加物品
        </Button>
      </div>
      {items.length > 0 && (
        <div className="flex shrink-0 items-center gap-3 pb-3">
          <Segmented
            ariaLabel="显示"
            value={onlyUnpacked ? "left" : "all"}
            onChange={(v) => setOnlyUnpacked(v === "left")}
            options={[
              { value: "all", label: `全部 ${t.total}` },
              { value: "left", label: `还没装 ${t.total - t.packed}` },
            ]}
          />
        </div>
      )}
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto pb-2">{itemsArea("清单是空的")}</div>
    </section>
  );

  const templateView = (t: Template) => (
    <section className="flex min-w-0 flex-1 flex-col">
      <div className="flex shrink-0 items-start justify-between gap-4 pb-3">
        <div className="min-w-0">
          <div className="flex items-center gap-1">
            <h2 className="truncate text-lg font-bold">{t.name}</h2>
            <IconButton label="编辑模板" onClick={() => setTemplateDialog("edit")}>
              <Pencil size={14} />
            </IconButton>
          </div>
          <p className="text-sm text-muted-foreground">{[`${t.itemCount} 件物品`, t.note].filter(Boolean).join(" · ")}</p>
        </div>
        <div className="flex shrink-0 gap-2">
          <Button onClick={() => void switchTo(() => setTripDialog({ mode: "new", templateId: t.id }))}>用它新建出行</Button>
          <Button variant="primary" onClick={() => void panel.request("new")}>
            <Plus size={14} /> 添加物品
          </Button>
        </div>
      </div>
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto pb-2">{itemsArea("这个模板还是空的")}</div>
    </section>
  );

  const loading = tripsQ.isPending || templatesQ.isPending;

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader icon={m.icon} color={m.color} title={m.label} right={stats} />

      <div className="flex shrink-0 items-center gap-3 px-8 pb-4">
        <Segmented
          ariaLabel="出行或模板"
          value={tab}
          onChange={(v) => void switchTo(() => setTab(v))}
          options={[
            { value: "trips", label: `出行 ${trips.length}` },
            { value: "templates", label: `模板 ${templates.length}` },
          ]}
        />
        <div className="flex-1" />
        <Button variant="ghost" className="text-muted-foreground" onClick={() => setManagingKinds(true)}>
          <Tags size={14} /> 管理类别
        </Button>
      </div>

      {loading ? null : tab === "trips" && trips.length === 0 ? (
        <EmptyState
          title="行李箱空空，说走就走"
          hint={templates.length > 0 ? "从模板新建一次出行，清单会自动复制过来" : "先在「模板」里建好常用清单，或者在「设置」里导入 Excel"}
          action={
            <Button variant="primary" onClick={() => setTripDialog({ mode: "new" })}>
              <Plus size={14} /> 新出行
            </Button>
          }
        />
      ) : tab === "templates" && templates.length === 0 ? (
        <EmptyState
          title="还没有模板"
          hint="把常去的场景（回家、出国…）的物品整理成模板，每次出行直接复制"
          action={
            <Button variant="primary" onClick={() => setTemplateDialog("new")}>
              <Plus size={14} /> 新模板
            </Button>
          }
        />
      ) : (
        <div className="flex min-h-0 flex-1 gap-6 px-8 pb-6">
          {tab === "trips" ? (
            <SideList
              label="出行"
              items={trips}
              currentId={trip?.id ?? null}
              onSelect={(id) => void switchTo(() => setTripId(id))}
              newLabel="新出行"
              onNew={() => void switchTo(() => setTripDialog({ mode: "new" }))}
              render={(t) => (
                <>
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="truncate">{t.name}</span>
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {t.packed}/{t.total}
                    </span>
                  </div>
                  <ProgressBar done={t.packed} total={t.total} />
                  <div className="text-xs text-muted-foreground">{formatDay(t.departOn) ?? "未定日期"}</div>
                </>
              )}
            />
          ) : (
            <SideList
              label="模板"
              items={templates}
              currentId={template?.id ?? null}
              onSelect={(id) => void switchTo(() => setTemplateId(id))}
              newLabel="新模板"
              onNew={() => void switchTo(() => setTemplateDialog("new"))}
              onReorder={(ids) => mut.reorderTemplates.mutate(ids)}
              render={(t, grip) => (
                <div className="flex items-baseline justify-between gap-2">
                  <span className="-ml-2 flex min-w-0 items-center gap-0.5">
                    {grip}
                    <span className="truncate">{t.name}</span>
                  </span>
                  <span className="shrink-0 text-xs text-muted-foreground">{t.itemCount} 件</span>
                </div>
              )}
            />
          )}
          {tab === "trips" ? trip && tripView(trip) : template && templateView(template)}
        </div>
      )}

      {parentId !== null && sel !== null && (sel === "new" || selectedItem) && (
        <ItemPanel
          key={`${list}-${sel}`}
          list={list}
          parentId={parentId}
          item={selectedItem}
          onClose={() => void panel.request(null)}
          onDone={panel.done}
          onDirtyChange={panel.setDirty}
          onManageKinds={() => setManagingKinds(true)}
        />
      )}
      {tripDialog && (
        <TripDialog
          trip={tripDialog.mode === "edit" ? trip : null}
          templates={templates}
          presetTemplateId={tripDialog.mode === "new" ? tripDialog.templateId : undefined}
          onClose={() => setTripDialog(null)}
          onCreated={(t) => {
            setTab("trips");
            setTripId(t.id);
            setOnlyUnpacked(false);
          }}
          onDeleted={() => setTripId(null)}
        />
      )}
      {templateDialog && (
        <NameDialog
          title={templateDialog === "edit" ? "编辑模板" : "新模板"}
          nameLabel="模板名"
          initial={templateDialog === "edit" ? template : null}
          onClose={() => setTemplateDialog(null)}
          onSubmit={async (patch) => {
            if (templateDialog === "edit" && template) {
              await mut.updateTemplate.mutateAsync({ id: template.id, patch });
              toast("已保存");
            } else {
              const t = await mut.createTemplate.mutateAsync(patch);
              toast(`已添加模板「${t.name}」`);
              setTemplateId(t.id);
            }
          }}
          remove={
            templateDialog === "edit" && template
              ? {
                  label: "删除模板",
                  confirmTitle: `删除模板「${template.name}」？`,
                  confirmMessage: `其中的 ${template.itemCount} 件物品会一起删除。用它建过的出行不受影响。`,
                  run: async () => {
                    await mut.deleteTemplate.mutateAsync(template.id);
                    toast(`已删除模板「${template.name}」`);
                    setTemplateId(null);
                  },
                }
              : undefined
          }
        />
      )}
      {managingKinds && (
        <KindManager
          module="packing"
          moduleLabel={m.label}
          unit="件"
          noun="类别"
          itemsKey={PACKING_KEY}
          onClose={() => setManagingKinds(false)}
        />
      )}
      {panel.confirmDialog}
    </div>
  );
}
