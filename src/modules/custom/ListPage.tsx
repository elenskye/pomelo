// 五花八门: one made list (route list/:id). Table of 名称 + the fields shown in it: click a header to sort
// (asc → desc → back), search 名称 and words, a filter per field (单选 / 多选 options, 勾选, 评分; all of
// them at once); unsorted and
// unfiltered rows drag into order. 勾选, 评分 and 单选 / 多选 (colored tags, shared/tags.tsx) change right
// in the table; a row opens the panel. Header ⋯:
// 改名和图标 · 管理字段 · 删除. Rules: lists.rs.
import { useMemo, useState, type MouseEvent } from "react";
import { Navigate, useParams } from "react-router";
import { ArrowDown, ArrowUp, Check, ChevronDown, Ellipsis, ExternalLink, Plus } from "lucide-react";
import { usePageContext } from "@/app/pageContext";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { useOpenRequest } from "@/shared/open";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { useSortable } from "@/shared/sortable";
import { StarRating } from "@/shared/StarRating";
import { TagMulti, TagSelect } from "@/shared/tags";
import { Button, EmptyState, PageHeader, SearchInput } from "@/shared/ui";
import { madeDef, TEMPLATE_COLOR, useCustomModules, useCustomMutations, useFields, useItems, type Custom, type Field, type Item, type Value } from "./api";
import { CustomDialog } from "./CustomDialog";
import { FieldManager } from "./FieldManager";
import { ItemPanel } from "./ItemPanel";

const collator = new Intl.Collator("zh-CN", { numeric: true });
type Sort = { field: number | "name"; desc: boolean } | null;

const host = (link: string) => {
  try {
    return new URL(/^[a-z][\w+.-]*:/i.test(link) ? link : `https://${link}`).hostname.replace(/^www\./, "") || link;
  } catch {
    return link;
  }
};

/** What a value reads as (search, sorting text). */
export function valueText(f: Field, v: Value | undefined): string {
  if (v === undefined) return "";
  if (Array.isArray(v)) return v.join("、");
  if (f.type === "check") return v ? "是" : "";
  return String(v);
}

/** Sort key: empty values always last. */
function compare(f: Field | "name", a: Item, b: Item): number {
  if (f === "name") return collator.compare(a.name, b.name);
  const x = a.values[f.id];
  const y = b.values[f.id];
  if (x === undefined || y === undefined) return x === undefined ? (y === undefined ? 0 : 1) : -1;
  switch (f.type) {
    case "number":
    case "rating":
      return Number(x) - Number(y);
    case "check":
      return 0;
    case "multi":
    case "select": {
      const rank = (v: Value) => f.options.indexOf(Array.isArray(v) ? v[0] : String(v));
      return rank(x) - rank(y);
    }
    default:
      return collator.compare(String(x), String(y));
  }
}

/** Per field id: an option, "yes" / "no" (勾选) or ">=<n>" (评分). */
type Filters = Record<string, string>;

/** The choices of a field that can filter (none: it can't). */
function choices(f: Field): { value: string; label: string }[] {
  if (f.type === "select" || f.type === "multi") return f.options.map((o) => ({ value: o, label: o }));
  if (f.type === "check") return [{ value: "yes", label: "是" }, { value: "no", label: "否" }];
  if (f.type === "rating") return [5, 4, 3].map((n) => ({ value: `>=${n}`, label: n === 5 ? "5 星" : `${n} 星以上` }));
  return [];
}

function passes(it: Item, field: Field, choice: string): boolean {
  const v = it.values[field.id];
  if (choice.startsWith(">=")) return typeof v === "number" && v >= Number(choice.slice(2));
  if (field.type === "check") return (v === true) === (choice === "yes");
  return Array.isArray(v) ? v.includes(choice) : v === choice;
}

function Cell(props: { f: Field; item: Item; onSet: (v: Value | null) => void }) {
  const { f } = props;
  const v = props.item.values[f.id];
  const toast = useToast();
  const stop = (e: MouseEvent) => e.stopPropagation();
  switch (f.type) {
    case "check":
      return (
        <button
          type="button"
          role="checkbox"
          aria-checked={v === true}
          aria-label={f.name}
          onClick={(e) => (stop(e), props.onSet(v === true ? null : true))}
          className={`flex size-4 items-center justify-center rounded-[4px] border outline-none focus-visible:ring-2 focus-visible:ring-ring ${v === true ? "border-primary bg-primary text-primary-foreground" : "bg-card hover:border-foreground/40"}`}
        >
          {v === true && <Check size={12} strokeWidth={2.5} />}
        </button>
      );
    case "rating":
      return <StarRating size={13} value={typeof v === "number" ? v : null} onChange={(n) => props.onSet(n)} />;
    case "select":
      return <TagSelect label={f.name} options={f.options.map((o) => ({ value: o, name: o }))} value={typeof v === "string" ? v : null} onChange={(o) => props.onSet(o)} />;
    case "multi":
      return <TagMulti label={f.name} options={f.options} value={Array.isArray(v) ? v : []} onChange={(xs) => props.onSet(xs.length ? xs : null)} />;
    case "link":
      return typeof v === "string" ? (
        <button
          type="button"
          title={v}
          onClick={(e) => (stop(e), void ipc.openLink(v).catch((err) => toast(errorText(err), "error")))}
          className="flex max-w-full items-center gap-0.5 rounded px-1 text-xs text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          <span className="truncate">{host(v)}</span>
          <ExternalLink size={11} className="shrink-0" />
        </button>
      ) : null;
    case "year":
      return typeof v === "string" ? <span className="tabular-nums">{v}</span> : null;
    case "number":
      return v === undefined ? null : <span className="tabular-nums">{String(v)}</span>;
    default:
      return <span className="block truncate">{typeof v === "string" ? v.split("\n")[0] : ""}</span>;
  }
}

function ListTable(props: { fields: Field[]; items: Item[]; selectedId: number | null; sort: Sort; onSort: (s: Sort) => void; onOpen: (id: number) => void; onReorder?: (ids: number[]) => void; moduleId: number }) {
  const m = useCustomMutations();
  const sort = useSortable<HTMLTableSectionElement>((ids) => props.onReorder?.(ids));
  const shown = props.fields.filter((f) => f.shown);
  const header = (key: number | "name", label: string, className = "") => {
    const on = props.sort?.field === key;
    const next: Sort = !on ? { field: key, desc: false } : !props.sort!.desc ? { field: key, desc: true } : null;
    return (
      <th key={String(key)} className={`px-3 py-2 text-left font-normal ${className}`}>
        <button type="button" onClick={() => props.onSort(next)} className={`inline-flex items-center gap-0.5 rounded outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring ${on ? "text-foreground" : ""}`}>
          {label}
          {on && (props.sort!.desc ? <ArrowDown size={12} /> : <ArrowUp size={12} />)}
        </button>
      </th>
    );
  };
  return (
    <div className="overflow-x-auto rounded-card border bg-card">
      <table className="w-full text-sm" style={{ minWidth: 240 + shown.length * 120 }}>
        <thead className="border-b text-xs text-muted-foreground">
          <tr>
            {header("name", "名称", "w-[28%]")}
            {shown.map((f) => header(f.id, f.name))}
          </tr>
        </thead>
        <tbody ref={sort.listRef}>
          {props.items.map((it) => (
            <tr
              key={it.id}
              onClick={() => props.onOpen(it.id)}
              {...sort.item(it.id)}
              {...(props.onReorder && props.items.length > 1 ? sort.handle(it.id) : {})}
              className={`cursor-pointer border-b transition-colors duration-150 last:border-b-0 ${props.selectedId === it.id ? "bg-accent" : "bg-card hover:bg-muted/70"}`}
            >
              <td className="max-w-0 truncate px-3 py-2">{it.name}</td>
              {shown.map((f) => (
                <td key={f.id} className="max-w-0 px-3 py-2">
                  <Cell f={f} item={it} onSet={(v) => m.itemUpdate.mutate({ id: it.id, patch: { values: { [f.id]: v } } })} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ListView(props: { list: Custom }) {
  const list = props.list;
  const def = madeDef(list);
  const { data: fields = [] } = useFields(list.id);
  const { data: items = [] } = useItems(list.id);
  const m = useCustomMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const panel = usePanelSelection<number | "new">();
  const [q, setQ] = useState("");
  const [filters, setFilters] = useState<Filters>({});
  const [sort, setSort] = useState<Sort>(null);
  const [dialog, setDialog] = useState<"rename" | "fields" | null>(null);
  useOpenRequest((h) => {
    if (h.kind === "list_item") {
      setFilters({});
      setQ("");
      void panel.request(h.id);
    }
  });
  usePageKeys({ onNew: () => void panel.request("new"), onEscape: panel.selection !== null ? () => void panel.request(null) : undefined });
  const filterable = fields.filter((f) => choices(f).length > 0);
  // the ones still valid (a field or option may have gone)
  const active = filterable.flatMap((f) => { const c = choices(f).find((x) => x.value === filters[f.id]); return c ? [{ field: f, choice: c }] : []; });

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    const hit = (it: Item) => !needle || [it.name, ...fields.map((f) => valueText(f, it.values[f.id]))].some((t) => t.toLowerCase().includes(needle));
    const xs = items.filter((it) => hit(it) && active.every((a) => passes(it, a.field, a.choice.value)));
    if (!sort) return xs;
    const f = sort.field === "name" ? "name" : fields.find((x) => x.id === sort.field);
    if (!f) return xs;
    // empty values stay last either way
    return [...xs].sort((a, b) => {
      const empty = (it: Item) => f !== "name" && it.values[f.id] === undefined;
      if (empty(a) !== empty(b)) return empty(a) ? 1 : -1;
      return sort.desc ? compare(f, b, a) : compare(f, a, b);
    });
  }, [items, fields, q, active, sort]); // eslint-disable-line react-hooks/exhaustive-deps

  const editing = typeof panel.selection === "number" ? items.find((i) => i.id === panel.selection) : undefined;
  const filterText = [...active.map((a) => `${a.field.name}：${a.choice.label}`), q.trim() ? `搜「${q.trim()}」` : null].filter(Boolean).join(" · ") || null;
  usePageContext({ module: list.key, filter: filterText, record: editing ? { kind: "list_item", id: editing.id, label: `${list.name}：${editing.name}` } : null });
  const canDrag = !sort && !active.length && !q.trim();

  const del = async () => {
    const ok = await confirm({ title: `删除「${list.name}」？`, message: items.length > 0 ? `里面的 ${items.length} 条也会一起删掉。` : undefined, confirmLabel: "删除", danger: true });
    if (ok) m.remove.mutate(list.id, { onSuccess: () => toast("已删除") });
  };

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader
        icon={def.icon}
        color={TEMPLATE_COLOR.list}
        title={list.name}
        right={
          <div className="flex items-center justify-end gap-2">
            <p className="pointer-events-none text-sm text-muted-foreground">
              共 <b className="text-base text-foreground">{items.length}</b> 条
            </p>
            <Dropdown<string | null>
              plain
              ariaLabel="更多"
              value={null}
              align="end"
              onChange={(a) => (a === "rename" || a === "fields" ? setDialog(a) : a === "delete" && void del())}
              options={[
                { value: "rename", label: "改名和图标…" },
                { value: "fields", label: "管理字段…" },
                { value: "delete", label: <span className="text-destructive">删除这个清单…</span> },
              ]}
              renderTrigger={() => <Ellipsis size={16} />}
              triggerClassName="rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
            />
          </div>
        }
      />
      <div className="flex min-h-0 flex-1 flex-col px-8 pb-6">
        <div className="flex shrink-0 items-center gap-2 pb-3">
          <div className="w-56">
            <SearchInput value={q} onChange={(e) => setQ(e.target.value)} placeholder="搜索" aria-label="搜索" />
          </div>
          {filterable.map((f) => {
            const on = active.find((a) => a.field.id === f.id);
            return (
              <Dropdown<string>
                key={f.id}
                ariaLabel={`按「${f.name}」筛选`}
                value={on?.choice.value ?? ""}
                onChange={(v) => setFilters((x) => ({ ...x, [f.id]: v }))}
                options={[{ value: "", label: "全部" }, ...choices(f)]}
                menuMaxHeight={360}
                renderTrigger={(_, open) => (
                  <>
                    <span className={on ? "" : "text-muted-foreground"}>{on ? `${f.name}：${on.choice.label}` : f.name}</span>
                    <ChevronDown size={14} className={`text-muted-foreground transition-transform duration-150 ${open ? "rotate-180" : ""}`} />
                  </>
                )}
              />
            );
          })}
          <div className="flex-1" />
          <Button variant="primary" onClick={() => void panel.request("new")}>
            <Plus size={14} /> 新条目
          </Button>
        </div>
        {items.length === 0 ? (
          <EmptyState title="清单还空着" action={fields.length === 0 ? <Button onClick={() => setDialog("fields")}>管理字段</Button> : undefined} />
        ) : shown.length === 0 ? (
          <p className="py-10 text-center text-muted-foreground">没有符合的条目</p>
        ) : (
          <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
            <ListTable
              fields={fields}
              items={shown}
              moduleId={list.id}
              selectedId={editing?.id ?? null}
              sort={sort}
              onSort={setSort}
              onOpen={(id) => void panel.request(id)}
              onReorder={canDrag ? (ids) => m.itemsReorder.mutate({ moduleId: list.id, ids }) : undefined}
            />
          </div>
        )}
      </div>
      {(panel.selection === "new" || editing) && (
        <ItemPanel key={String(panel.selection)} moduleId={list.id} fields={fields} item={editing ?? null} onClose={() => void panel.request(null)} onDone={panel.done} onDirtyChange={panel.setDirty} />
      )}
      {dialog === "rename" && <CustomDialog template="list" custom={list} onClose={() => setDialog(null)} />}
      {dialog === "fields" && <FieldManager moduleId={list.id} fields={fields} itemCount={items.length} onClose={() => setDialog(null)} />}
      {panel.confirmDialog}
      {confirmDialog}
    </div>
  );
}

export default function ListPage() {
  const { id } = useParams();
  const { data, isSuccess } = useCustomModules();
  const list = data?.find((c) => c.template === "list" && c.id === Number(id));
  if (!list) return isSuccess ? <Navigate to="/" replace /> : null;
  return <ListView key={list.id} list={list} />;
}
