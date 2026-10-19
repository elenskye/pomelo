// 芝麻开门: set the master password → unlock (Touch ID / master password) → the list. The list shows
// no passwords; copy buttons hand the field from Rust straight to the pasteboard. ⌘N adds one.
// Order: by name until the first drag (whole row, not while searching), then the hand-made one.
// Views: 全部 · each category · 未分类 · 归档 (archived ones only there; a search also finds the
// archived ones of the view). A drag inside a view is sent as the whole list (Rust's first-drag rule).
import { useEffect, useMemo, useState, type MouseEvent, type ReactNode } from "react";
import { Copy, ExternalLink, Lock, Plus } from "lucide-react";
import { moduleById } from "@/app/modules";
import { errorText, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { KindEditor } from "@/shared/kinds/KindManager";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { useSortable } from "@/shared/sortable";
import { Button, PageHeader, SearchInput, Segmented } from "@/shared/ui";
import { useVaultCategories, useVaultList, useVaultMutations, useVaultStatus, type CopyField, type Summary } from "./api";
import { EntryPanel } from "./EntryPanel";
import { Setup, Unlock } from "./Gate";

const collator = new Intl.Collator("zh-Hans-CN-u-co-pinyin", { numeric: true, sensitivity: "base" });
const FIELD_NAME: Record<CopyField, string> = { username: "用户名", account: "账号", password: "密码", link: "链接" };

/** "all" · "none" (未分类) · "archived" · `c<id>` (a category). */
type View = string;
const catView = (id: number): View => `c${id}`;

/** Hostname for the table (full link stays in the panel). */
function host(link: string) {
  try {
    return new URL(/^[a-z][a-z0-9+.-]*:/i.test(link) ? link : `https://${link}`).hostname.replace(/^www\./, "");
  } catch {
    return link;
  }
}

function CopyCell(props: { text: ReactNode; onCopy?: () => void; label: string }) {
  return (
    <div className="group/cell flex min-w-0 items-center gap-1">
      <span className="min-w-0 truncate">{props.text}</span>
      {props.onCopy && (
        <button
          type="button"
          aria-label={`复制${props.label}`}
          title={`复制${props.label}`}
          onClick={(e: MouseEvent) => {
            e.stopPropagation();
            props.onCopy!();
          }}
          className="shrink-0 rounded p-1 text-muted-foreground opacity-0 outline-none group-hover/cell:opacity-100 hover:bg-muted hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Copy size={13} />
        </button>
      )}
    </div>
  );
}

function List(props: { rows: Summary[]; selectedId: number | null; markArchived: boolean; onOpen: (id: number) => void; onReorder?: (ids: number[]) => void }) {
  const m = useVaultMutations();
  const toast = useToast();
  const sort = useSortable<HTMLTableSectionElement>((ids) => props.onReorder?.(ids));
  const copy = (id: number, field: CopyField) => m.copy.mutate({ id, field }, { onSuccess: () => toast(`已复制${FIELD_NAME[field]}`) });
  if (props.rows.length === 0) return null;
  return (
    <table className="w-full table-fixed overflow-hidden rounded-card border bg-card text-sm">
      <thead className="border-b text-xs text-muted-foreground">
        <tr>
          <th className="w-[30%] px-3 py-2 text-left font-normal">名称</th>
          <th className="px-3 py-2 text-left font-normal">用户名</th>
          <th className="px-3 py-2 text-left font-normal">账号</th>
          <th className="w-36 px-3 py-2 text-left font-normal">密码</th>
        </tr>
      </thead>
      <tbody ref={sort.listRef}>
        {props.rows.map((r) => (
          <tr
            key={r.id}
            onClick={() => props.onOpen(r.id)}
            {...sort.item(r.id)}
            {...(props.onReorder && props.rows.length > 1 ? sort.handle(r.id) : {})}
            className={`cursor-pointer border-b transition-colors duration-150 last:border-b-0 ${props.selectedId === r.id ? "bg-accent" : "bg-card hover:bg-muted/70"}`}
          >
            <td className="px-3 py-2">
              <div className="flex min-w-0 items-center gap-1.5">
                <span className="min-w-0 truncate">{r.name}</span>
                {r.link && (
                  <button
                    type="button"
                    title={r.link}
                    aria-label={`打开 ${host(r.link)}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      void ipc.openLink(r.link!).catch((err) => toast(errorText(err), "error"));
                    }}
                    className="flex shrink-0 items-center gap-0.5 rounded px-1 text-xs text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    {host(r.link)}
                    <ExternalLink size={11} />
                  </button>
                )}
                {props.markArchived && r.archived && <span className="shrink-0 text-xs text-muted-foreground/70">已归档</span>}
              </div>
            </td>
            <td className="px-3 py-2">
              <CopyCell label="用户名" text={r.username} onCopy={r.username ? () => copy(r.id, "username") : undefined} />
            </td>
            <td className="px-3 py-2">
              <CopyCell label="账号" text={r.account} onCopy={r.account ? () => copy(r.id, "account") : undefined} />
            </td>
            <td className="px-3 py-2">
              <CopyCell
                label="密码"
                text={r.hasPassword ? <span className="tracking-widest text-muted-foreground">••••••••</span> : null}
                onCopy={r.hasPassword ? () => copy(r.id, "password") : undefined}
              />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function OpenVault() {
  const listQ = useVaultList(true);
  const cats = useVaultCategories(true).data ?? [];
  // never arranged by hand (every sortOrder 0): by name; else Rust's order
  const all = useMemo(() => {
    const rows = [...(listQ.data ?? [])];
    return rows.some((r) => r.sortOrder > 0) ? rows : rows.sort((a, b) => collator.compare(a.name, b.name));
  }, [listQ.data]);
  const [q, setQ] = useState("");
  const [picked, setView] = useState<View>("all");
  const [managing, setManaging] = useState(false);
  const panel = usePanelSelection<number | "new">();
  const m = useVaultMutations();
  usePageKeys({ onNew: () => void panel.request("new"), onEscape: panel.selection !== null ? () => void panel.request(null) : undefined });

  const live = all.filter((r) => !r.archived);
  const count = (v: View) => (v === "archived" ? all.length - live.length : live.filter((r) => inView(r, v, false)).length);
  // a deleted category, or an emptied 未分类 / 归档, falls back to 全部
  const view = picked === "all" || cats.some((c) => catView(c.id) === picked) || ((picked === "none" || picked === "archived") && count(picked) > 0) ? picked : "all";
  const needle = q.trim().toLowerCase();
  const shown = all.filter((r) => inView(r, view, !!needle) && (!needle || [r.name, r.link, r.username, r.account].some((s) => s?.toLowerCase().includes(needle))));
  const tabs = [
    { value: "all", label: "全部" },
    ...cats.map((c) => ({ value: catView(c.id), label: c.name })),
    ...(count("none") > 0 ? [{ value: "none", label: "未分类" }] : []),
    ...(count("archived") > 0 ? [{ value: "archived", label: "归档" }] : []),
  ];
  const lock = async () => {
    if (await panel.request(null)) m.lock.mutate();
  };
  // the dragged view's rows take their own slots in the whole list
  const reorder = (ids: number[]) => {
    const moved = new Set(ids);
    const queue = [...ids];
    m.reorder.mutate(all.map((r) => (moved.has(r.id) ? queue.shift()! : r.id)));
  };

  return (
    <>
      <div className="flex min-h-0 flex-1 flex-col px-8 pb-6">
        <div className="flex shrink-0 items-center justify-between gap-4 pb-3">
          <div className="w-64">
            <SearchInput value={q} onChange={(e) => setQ(e.target.value)} placeholder="搜索" aria-label="搜索" />
          </div>
          <div className="flex gap-2">
            <Button onClick={() => void lock()}>
              <Lock size={14} /> 锁上
            </Button>
            <Button variant="primary" onClick={() => void panel.request("new")}>
              <Plus size={14} /> 新密码
            </Button>
          </div>
        </div>
        {all.length > 0 && (
          <div className="flex shrink-0 items-center gap-3 pb-3">
            <div className="-m-1 min-w-0 overflow-x-auto p-1">
              <Segmented
                ariaLabel="分类"
                value={view}
                onChange={setView}
                options={tabs.map((t) => ({
                  value: t.value,
                  label: (
                    <>
                      {t.label} <span className="text-xs text-muted-foreground">{count(t.value)}</span>
                    </>
                  ),
                }))}
              />
            </div>
            <div className="flex-1" />
            <Button variant="ghost" onClick={() => setManaging(true)} className="shrink-0 text-muted-foreground">
              管理分类
            </Button>
          </div>
        )}
        <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
          <List
            rows={shown}
            selectedId={typeof panel.selection === "number" ? panel.selection : null}
            markArchived={view !== "archived"}
            onOpen={(id) => void panel.request(id)}
            onReorder={needle ? undefined : reorder}
          />
          {listQ.data && shown.length === 0 && <p className="py-10 text-center text-muted-foreground">{all.length === 0 ? "芝麻还没开过门" : "没有找到"}</p>}
        </div>
      </div>
      {panel.selection !== null && (
        <EntryPanel
          key={String(panel.selection)}
          id={panel.selection === "new" ? null : panel.selection}
          categories={cats}
          defaultCategory={cats.find((c) => catView(c.id) === view)?.id ?? null}
          onManageCategories={() => setManaging(true)}
          onClose={() => void panel.request(null)}
          onDone={panel.done}
          onDirtyChange={panel.setDirty}
        />
      )}
      {managing && (
        <KindEditor
          noun="分类"
          unit="个"
          moduleLabel="芝麻开门"
          kinds={cats.map((c) => ({ id: c.id, name: c.name, count: all.filter((r) => r.categoryId === c.id).length }))}
          ops={{ create: m.categoryCreate.mutateAsync, rename: m.categoryRename.mutateAsync, remove: m.categoryDelete.mutateAsync, reorder: m.categoryReorder.mutateAsync }}
          onClose={() => setManaging(false)}
        />
      )}
      {panel.confirmDialog}
    </>
  );
}

/** In view `v`; `withArchived`: a search also finds the archived ones of the view. */
function inView(r: Summary, v: View, withArchived: boolean) {
  if (v === "archived") return r.archived;
  if (r.archived && !withArchived) return false;
  return v === "all" || (v === "none" ? r.categoryId === null : catView(r.categoryId ?? -1) === v);
}

export default function VaultPage() {
  const mod = moduleById("vault");
  const status = useVaultStatus().data;
  const listQ = useVaultList(status?.state === "open");
  // Touch ID starts by itself only for a locked vault met on arrival (not after 锁上 / the idle lock here)
  const [arrived, setArrived] = useState(true);
  useEffect(() => {
    if (status?.state === "open") setArrived(false);
  }, [status?.state]);
  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader
        icon={mod.icon}
        color={mod.color}
        title={mod.label}
        right={
          status?.state === "open" && listQ.data ? (
            <p className="pointer-events-none text-sm text-muted-foreground">
              共 <b className="text-base text-foreground">{listQ.data.length}</b> 个
            </p>
          ) : null
        }
      />
      {status?.state === "new" && <Setup color={mod.color} />}
      {status?.state === "locked" && <Unlock color={mod.color} status={status} autoTouch={arrived} />}
      {status?.state === "open" && <OpenVault />}
    </div>
  );
}
