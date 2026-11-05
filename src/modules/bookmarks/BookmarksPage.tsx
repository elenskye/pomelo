// 一网打尽 (网址收藏夹; Rust modules/bookmarks.rs): left the folder tree (一网打尽 = the top level; arrows fold,
// remembered), right what's in the chosen folder — folders and links in their own order, like Edge's 收藏夹.
// A link opens in the browser, a folder opens here. The circles pick (⌘-click and ⇧-click too); with some picked
// a bar offers 打开 · 移到… · 删除 (and a click picks instead of opening). Drag a row — or the picked ones —
// between rows to reorder, onto a folder (in the list or the tree) to move it in (pointer events, as
// notes/TagTree). The tree's folders drag too: a folder onto a tree row's edge goes before / after it (among
// that row's siblings), onto its middle goes in; links onto a tree row always go in.
// 搜索 lists matches from every folder with where they are. ⋯ / right-click: 打开 · 编辑 · 放首页
// · 移到… · 删除. ⌘N adds a link, ⌘F searches. Every change is one undo step (Rust).
import { useEffect, useMemo, useRef, useState, type FormEvent, type MouseEvent as ReactMouseEvent, type PointerEvent as ReactPointerEvent } from "react";
import { Check, ChevronRight, ExternalLink, Folder, FolderInput, FolderPlus, House, Pencil, Plus, Trash2 } from "lucide-react";
import { Menu, type MenuAt, type MenuRow } from "@/app/Menu";
import { moduleById } from "@/app/modules";
import { usePageContext } from "@/app/pageContext";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { Modal } from "@/shared/Modal";
import { useOpenRequest, type Hit } from "@/shared/open";
import { usePageKeys } from "@/shared/panel";
import { isStringArray, useSetting } from "@/shared/settings";
import { Button, EmptyState, Field, PageHeader, SearchInput, Switch, TextInput } from "@/shared/ui";
import { buildTree, linksIn, pathTo, useBookmarkMutations, useBookmarks, useSiteIcons, type Node } from "./api";
import { SiteIcon } from "./SiteIcon";

const OPEN_KEY = "bookmarks.open";
const THRESHOLD = 4;
const ROOT = "一网打尽";

/** Where a drag would land: into a folder (null: the top level), or before / after a row (among its siblings). */
type Drop ={ kind: "into"; folder: number | null } | { kind: "before" | "after"; row: number };
type Drag = { ids: number[]; x: number; y: number; drop: Drop | null };
type Hint = "before" | "after" | "into" | null;
type TreeDrag = { onPointerDown: (e: ReactPointerEvent<HTMLElement>) => void; onClickCapture: (e: ReactMouseEvent) => void; dragging: boolean };
type Editing = { mode: "new"; kind: Node["kind"]; parentId: number | null } | { mode: "edit"; node: Node };

export default function BookmarksPage() {
  const m = moduleById("bookmarks");
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const { data: list = [] } = useBookmarks();
  const icons = useSiteIcons();
  const mut = useBookmarkMutations();
  const { roots, byId } = useMemo(() => buildTree(list), [list]);
  const [folder, setFolder] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<number[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [menu, setMenu] = useState<MenuAt | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [flash, setFlash] = useState<number | null>(null);
  const [pending, setPending] = useState<Hit | null>(null);
  const [open, setOpen] = useSetting<string[]>(OPEN_KEY, [], isStringArray);
  const anchor = useRef<number | null>(null);
  const dragged = useRef(false);
  const search = useRef<HTMLInputElement>(null);

  // a folder that's gone (deleted, undone): back to the top level
  const current = folder != null && byId.get(folder)?.kind === "folder" ? folder : null;
  const path = pathTo(byId, current);
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const shown: { n: Node; where: string | null }[] = words.length
    ? list
        .map((b) => byId.get(b.id)!)
        .map((n) => ({ n, path: pathTo(byId, n.parentId).map((p) => p.title) }))
        .filter(({ n, path }) => words.every((w) => `${n.title} ${n.url} ${path.join(" ")}`.toLowerCase().includes(w)))
        .map(({ n, path }) => ({ n, where: path.length ? path.join(" / ") : ROOT }))
    : (current == null ? roots : (byId.get(current)?.kids ?? [])).map((n) => ({ n, where: null }));
  const links = list.filter((b) => b.kind === "link").length;
  const pickedNodes = picked.map((id) => byId.get(id)).filter((n): n is Node => !!n);

  // picking is per view
  useEffect(() => setPicked([]), [current, query]);
  // opened from ⌘K / 首页 / 柚子: its folder, the row flashing
  useOpenRequest((h) => setPending(h));
  useEffect(() => {
    if (!pending || !list.length) return;
    const n = byId.get(pending.id);
    setPending(null);
    if (!n) return;
    setQuery("");
    setFolder(n.kind === "folder" ? n.id : n.parentId);
    if (n.kind === "link") setFlash(n.id);
  }, [pending, list, byId]);
  useEffect(() => {
    if (flash == null) return;
    document.querySelector(`[data-row="${flash}"]`)?.scrollIntoView({ block: "nearest" });
    const t = window.setTimeout(() => setFlash(null), 1600);
    return () => window.clearTimeout(t);
  }, [flash, current]);

  const editingNode = editing?.mode === "edit" ? editing.node : null;
  usePageContext({
    module: "bookmarks",
    page: words.length ? "搜索" : path.length ? path.map((p) => p.title).join(" / ") : ROOT,
    filter: words.length ? query.trim() : null,
    record: editingNode ? { kind: "bookmark", id: editingNode.id, label: `网址：${editingNode.title}` } : null,
    selected: picked.length ? { kind: "bookmark", ids: picked, label: `选中的 ${picked.length} 项` } : null,
  });
  usePageKeys({
    onNew: () => setEditing({ mode: "new", kind: "link", parentId: current }),
    onFind: () => search.current?.focus(),
    onEscape: picked.length ? () => setPicked([]) : undefined,
  });

  const go = (id: number | null) => {
    setQuery("");
    setFolder(id);
  };
  const openLink = (url: string) => void ipc.openLink(url).catch((e) => toast(errorText(e), "error"));
  const isOpen = (id: number) => open.includes(String(id)) || path.some((p) => p.id === id && p.id !== current);
  const toggleOpen = (id: number) => setOpen(open.includes(String(id)) ? open.filter((x) => x !== String(id)) : [...open, String(id)]);

  // picking: a circle or ⌘-click toggles one, ⇧-click a range from the last one
  const pick = (id: number, range: boolean) => {
    const ids = shown.map((s) => s.n.id);
    const at = ids.indexOf(id);
    if (range && anchor.current != null && ids.includes(anchor.current)) {
      const from = ids.indexOf(anchor.current);
      const span = ids.slice(Math.min(from, at), Math.max(from, at) + 1);
      setPicked((p) => [...new Set([...p, ...span])]);
    } else {
      setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id]));
    }
    anchor.current = id;
  };
  const activate = (n: Node, e: ReactMouseEvent) => {
    if (e.metaKey || e.shiftKey || picked.length) return pick(n.id, e.shiftKey);
    if (n.kind === "folder") go(n.id);
    else openLink(n.url);
  };

  /** In the order they're shown (picked ones elsewhere after them). */
  const ordered = (ids: number[]) => [...shown.map((s) => s.n.id).filter((id) => ids.includes(id)), ...ids.filter((id) => !shown.some((s) => s.n.id === id))];
  const under = (id: number | null, ids: number[]) => {
    for (let at = id == null ? undefined : byId.get(id); at; at = at.parentId == null ? undefined : byId.get(at.parentId)) if (ids.includes(at.id)) return true;
    return false;
  };
  const place = (ids: number[], parentId: number | null, before: number | null) => mut.place.mutate({ ids, parentId, before });
  const moveTo = (ids: number[], target: number | null) => {
    place(ids, target, null);
    setPicked([]);
    if (target !== current && ids.length) toast(`移到「${target == null ? ROOT : byId.get(target)?.title}」了`);
  };
  const remove = async (nodes: Node[]) => {
    if (!nodes.length) return;
    const inside = nodes.reduce((s, n) => s + (n.kind === "folder" ? linksIn(n) : 0), 0);
    const ok = await confirm({
      title: nodes.length === 1 ? `删除「${nodes[0].title}」？` : `删除 ${nodes.length} 项？`,
      message: inside ? `文件夹里的 ${inside} 个网址也一起删。` : undefined,
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    mut.remove.mutate(nodes.map((n) => n.id));
    setPicked([]);
  };

  /** 移到…: the top level and every folder (not the moving ones or what's under them), as a menu. */
  const moveMenu = (x: number, y: number, ids: number[]) => {
    const rows: MenuRow[] = [{ head: "移到" }, { label: ROOT, icon: <Folder size={14} />, checked: false, onSelect: () => moveTo(ids, null) }];
    const walk = (ns: Node[], depth: number) =>
      ns.forEach((n) => {
        if (n.kind !== "folder" || ids.includes(n.id)) return;
        rows.push({ label: `${"　".repeat(depth)}${n.title}`, icon: <Folder size={14} />, onSelect: () => moveTo(ids, n.id) });
        walk(n.kids, depth + 1);
      });
    walk(roots, 1);
    setMenu({ x, y, rows });
  };
  const rowMenu = (n: Node, x: number, y: number) => {
    const ids = picked.includes(n.id) ? ordered(picked) : [n.id];
    const nodes = ids.map((id) => byId.get(id)).filter((v): v is Node => !!v);
    const rows: MenuRow[] =
      ids.length > 1
        ? [
            { label: `打开 ${nodes.filter((v) => v.kind === "link").length} 个网址`, icon: <ExternalLink size={14} />, onSelect: () => nodes.forEach((v) => v.kind === "link" && openLink(v.url)) },
            { label: "移到…", icon: <FolderInput size={14} />, onSelect: () => moveMenu(x, y, ids) },
            "-",
            { label: `删除 ${ids.length} 项`, icon: <Trash2 size={14} />, onSelect: () => void remove(nodes) },
          ]
        : [
            n.kind === "link" ? { label: "在浏览器打开", icon: <ExternalLink size={14} />, onSelect: () => openLink(n.url) } : { label: "打开", icon: <Folder size={14} />, onSelect: () => go(n.id) },
            { label: n.kind === "link" ? "编辑" : "改名", icon: <Pencil size={14} />, onSelect: () => setEditing({ mode: "edit", node: n }) },
            ...(n.kind === "link" ? [{ label: "放首页", icon: <House size={14} />, checked: n.onHome, onSelect: () => mut.update.mutate({ id: n.id, patch: { onHome: !n.onHome } }) }] : []),
            { label: "移到…", icon: <FolderInput size={14} />, onSelect: () => moveMenu(x, y, ids) },
            "-",
            { label: "删除", icon: <Trash2 size={14} />, onSelect: () => void remove([n]) },
          ];
    setMenu({ x, y, rows });
  };

  // ---- dragging (pointer events) ----
  const dropAt = (ids: number[], x: number, y: number): Drop | null => {
    const el = document.elementFromPoint(x, y) as HTMLElement | null;
    const target = el?.closest<HTMLElement>("[data-drop-folder], [data-row]");
    if (!target) return null;
    if (target.dataset.dropFolder !== undefined) {
      const f = target.dataset.dropFolder === "root" ? null : Number(target.dataset.dropFolder);
      if (under(f, ids)) return null;
      const n = f == null ? undefined : byId.get(f);
      if (!n || target.dataset.tree === undefined || !ids.every((id) => byId.get(id)?.kind === "folder")) return { kind: "into", folder: f };
      const b = target.getBoundingClientRect();
      const rel = (y - b.top) / b.height;
      if (rel >= 0.25 && rel <= 0.75) return { kind: "into", folder: f };
      if (rel > 0.75 && n.kids.some((k) => k.kind === "folder") && isOpen(n.id)) {
        // an open folder: right below it is its first subfolder
        const first = n.kids.find((k) => k.kind === "folder" && !ids.includes(k.id));
        if (first) return { kind: "before", row: first.id };
      }
      return under(n.parentId, ids) ? null : { kind: rel < 0.25 ? "before" : "after", row: n.id };
    }
    const row = Number(target.dataset.row);
    const n = byId.get(row);
    if (!n || ids.includes(row)) return null;
    const b = target.getBoundingClientRect();
    const rel = (y - b.top) / b.height;
    if (n.kind === "folder" && (words.length || (rel > 0.25 && rel < 0.75))) return under(n.id, ids) ? null : { kind: "into", folder: n.id };
    if (words.length) return null; // search results come from many folders: no reordering there
    return { kind: rel < 0.5 ? "before" : "after", row };
  };
  const finish = (ids: number[], d: Drop) => {
    if (d.kind === "into") return moveTo(ids, d.folder);
    const parent = byId.get(d.row)?.parentId ?? null;
    const rows = (parent == null ? roots : (byId.get(parent)?.kids ?? [])).map((n) => n.id).filter((id) => !ids.includes(id));
    const before = d.kind === "before" ? d.row : (rows[rows.indexOf(d.row) + 1] ?? null);
    place(ids, parent, before);
    if (ids.some((id) => (byId.get(id)?.parentId ?? null) !== parent)) {
      setPicked([]);
      toast(`移到「${parent == null ? ROOT : byId.get(parent)?.title}」了`);
    }
  };
  /** `skip`: what inside the row is not a drag handle. */
  const startDrag = (n: Node, skip: string) => (e: ReactPointerEvent<HTMLElement>) => {
    dragged.current = false;
    if (e.button !== 0 || (e.target as HTMLElement).closest(skip)) return;
    const ids = picked.includes(n.id) ? ordered(picked) : [n.id];
    const sx = e.clientX;
    const sy = e.clientY;
    let cur: Drag | null = null;
    const move = (ev: PointerEvent) => {
      if (!cur && Math.hypot(ev.clientX - sx, ev.clientY - sy) < THRESHOLD) return;
      if (!cur) window.getSelection()?.removeAllRanges();
      cur = { ids, x: ev.clientX, y: ev.clientY, drop: dropAt(ids, ev.clientX, ev.clientY) };
      setDrag(cur);
    };
    const up = () => {
      stop();
      if (!cur) return;
      dragged.current = true;
      setDrag(null);
      if (cur.drop) finish(ids, cur.drop);
    };
    const key = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      ev.stopPropagation();
      stop();
      if (cur) dragged.current = true;
      setDrag(null);
    };
    const cancel = () => {
      stop();
      setDrag(null);
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key, true);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", key, true);
  };
  const swallow = (e: ReactMouseEvent) => {
    if (!dragged.current) return false;
    dragged.current = false;
    e.preventDefault();
    e.stopPropagation();
    return true;
  };
  const hint = (id: number | null): Hint => {
    const d = drag?.drop;
    if (!d) return null;
    if (d.kind === "into") return d.folder === id ? "into" : null;
    return d.row === id ? d.kind : null;
  };
  const treeDrag = (n: Node) => ({
    onPointerDown: startDrag(n, "[data-fold]"),
    onClickCapture: (e: ReactMouseEvent) => void swallow(e),
    dragging: !!drag?.ids.includes(n.id),
  });

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      {confirmDialog}
      <PageHeader
        icon={m.icon}
        color={m.color}
        title={m.label}
        right={
          <p className="pointer-events-none text-sm text-muted-foreground">
            共 <b className="text-base text-foreground">{links}</b> 个网址
          </p>
        }
      />
      <div className="flex min-h-0 flex-1 gap-5 px-8 pb-6">
        <nav aria-label="文件夹" className="selectable -m-1 w-56 shrink-0 overflow-y-auto p-1">
          <TreeRow label={ROOT} depth={0} on={!words.length && current == null} hint={hint(null)} drop="root" count={links} onClick={() => go(null)} />
          <FolderTree nodes={roots} depth={1} current={words.length ? null : current} isOpen={isOpen} toggle={toggleOpen} go={go} hint={hint} drag={treeDrag} />
        </nav>
        <main className="flex min-w-0 flex-1 flex-col">
          <div className="flex shrink-0 flex-wrap items-center gap-2 pb-3">
            {picked.length ? (
              <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2 text-sm">
                <span className="font-bold tabular-nums">已选 {picked.length} 项</span>
                <Button onClick={() => pickedNodes.forEach((n) => n.kind === "link" && openLink(n.url))} disabled={!pickedNodes.some((n) => n.kind === "link")}>
                  <ExternalLink size={14} /> 打开
                </Button>
                <Button onClick={(e) => { const b = e.currentTarget.getBoundingClientRect(); moveMenu(b.left, b.bottom + 4, ordered(picked)); }}>
                  <FolderInput size={14} /> 移到…
                </Button>
                <Button onClick={() => void remove(pickedNodes)}>
                  <Trash2 size={14} /> 删除
                </Button>
                <Button variant="ghost" onClick={() => setPicked([])}>取消</Button>
              </div>
            ) : (
              <nav aria-label="位置" className="flex min-w-0 flex-1 flex-wrap items-center gap-1 text-sm">
                {words.length ? (
                  <span className="text-muted-foreground">找到 {shown.length} 条</span>
                ) : (
                  <>
                    <button type="button" data-drop-folder="root" onClick={() => go(null)} className={`rounded px-1 ${current == null ? "font-bold" : "text-muted-foreground hover:text-foreground"}`}>
                      {ROOT}
                    </button>
                    {path.map((p, i) => (
                      <span key={p.id} className="flex min-w-0 items-center gap-1">
                        <ChevronRight size={13} className="shrink-0 text-muted-foreground" />
                        <button type="button" data-drop-folder={p.id} onClick={() => go(p.id)} className={`truncate rounded px-1 ${i === path.length - 1 ? "font-bold" : "text-muted-foreground hover:text-foreground"}`}>
                          {p.title}
                        </button>
                      </span>
                    ))}
                  </>
                )}
              </nav>
            )}
            <div className="w-56">
              <SearchInput ref={search} placeholder="搜索" aria-label="搜索网址" value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === "Escape" && (e.stopPropagation(), setQuery(""))} />
            </div>
            <Button onClick={() => setEditing({ mode: "new", kind: "folder", parentId: current })}>
              <FolderPlus size={14} /> 新文件夹
            </Button>
            <Button variant="primary" onClick={() => setEditing({ mode: "new", kind: "link", parentId: current })}>
              <Plus size={14} /> 新网址
            </Button>
          </div>
          <div className="selectable -m-1 min-h-0 flex-1 overflow-y-auto p-1">
            {shown.length ? (
              <ul className="divide-y rounded-card border bg-card">
                {shown.map(({ n, where }) => (
                  <Row
                    key={n.id}
                    n={n}
                    where={where}
                    icons={icons}
                    picked={picked.includes(n.id)}
                    picking={picked.length > 0}
                    hint={hint(n.id)}
                    flash={flash === n.id}
                    dragging={!!drag?.ids.includes(n.id)}
                    onPointerDown={startDrag(n, "button, input")}
                    onClick={(e) => swallow(e) || activate(n, e)}
                    onPick={(range) => pick(n.id, range)}
                    onMenu={(x, y) => rowMenu(n, x, y)}
                  />
                ))}
              </ul>
            ) : (
              <EmptyState title={words.length ? "没有找到" : list.length ? "这个文件夹是空的" : "还没有收藏网址"} />
            )}
          </div>
        </main>
      </div>
      {drag && drag.drop && (
        <div className="pointer-events-none fixed z-50 max-w-56 truncate rounded-control border bg-popover px-2 py-0.5 text-sm shadow-md" style={{ left: drag.x + 12, top: drag.y + 10 }}>
          {drag.ids.length === 1 ? byId.get(drag.ids[0])?.title : `${drag.ids.length} 项`}
        </div>
      )}
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
      {editing && <EditDialog editing={editing} onClose={() => setEditing(null)} />}
    </div>
  );
}

function TreeRow(props: { label: string; depth: number; on: boolean; hint: Hint; drop: string; count: number; kids?: { open: boolean; toggle: () => void }; drag?: TreeDrag; onClick: () => void }) {
  const inset = Math.max(0, props.depth - 1) * 14;
  return (
    <div
      data-drop-folder={props.drop}
      data-tree={props.drag ? "" : undefined}
      onPointerDown={props.drag?.onPointerDown}
      onClickCapture={props.drag?.onClickCapture}
      onDragStart={(e) => e.preventDefault()}
      className={`relative flex items-center rounded-control text-sm transition-colors duration-150 ${props.hint === "into" ? "bg-accent ring-2 ring-primary/50" : props.on ? "bg-accent font-bold" : "hover:bg-muted/70"} ${props.drag?.dragging ? "opacity-40" : ""}`}
      style={{ paddingLeft: inset }}
    >
      {props.hint === "before" && <span className="absolute right-1 -top-px h-0.5 rounded-full bg-primary" style={{ left: inset + 6 }} />}
      {props.hint === "after" && <span className="absolute right-1 -bottom-px h-0.5 rounded-full bg-primary" style={{ left: inset + 6 }} />}
      {props.depth > 0 ? (
        <button
          type="button"
          data-fold
          aria-label={props.kids?.open ? "收起" : "展开"}
          disabled={!props.kids}
          onClick={props.kids?.toggle}
          className="grid size-6 shrink-0 place-items-center rounded text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:invisible"
        >
          <ChevronRight size={13} className={`transition-transform duration-150 ${props.kids?.open ? "rotate-90" : ""}`} />
        </button>
      ) : (
        <span className="w-1" />
      )}
      <button type="button" onClick={props.onClick} className="flex min-w-0 flex-1 items-center gap-1.5 py-1.5 pr-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring rounded-control">
        <Folder size={14} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate">{props.label}</span>
        {props.count > 0 && <span className="shrink-0 text-xs font-normal text-muted-foreground tabular-nums">{props.count}</span>}
      </button>
    </div>
  );
}

function FolderTree(props: { nodes: Node[]; depth: number; current: number | null; isOpen: (id: number) => boolean; toggle: (id: number) => void; go: (id: number) => void; hint: (f: number) => Hint; drag: (n: Node) => TreeDrag }) {
  return (
    <>
      {props.nodes
        .filter((n) => n.kind === "folder")
        .map((n) => {
          const sub = n.kids.some((k) => k.kind === "folder");
          const open = sub && props.isOpen(n.id);
          return (
            <div key={n.id}>
              <TreeRow label={n.title} depth={props.depth} on={props.current === n.id} hint={props.hint(n.id)} drop={String(n.id)} count={linksIn(n)} kids={sub ? { open, toggle: () => props.toggle(n.id) } : undefined} drag={props.drag(n)} onClick={() => props.go(n.id)} />
              {open && <FolderTree {...props} nodes={n.kids} depth={props.depth + 1} />}
            </div>
          );
        })}
    </>
  );
}

function Row(props: {
  n: Node;
  where: string | null;
  icons: Record<string, string>;
  picked: boolean;
  picking: boolean;
  hint: Hint;
  flash: boolean;
  dragging: boolean;
  onPointerDown: (e: ReactPointerEvent<HTMLElement>) => void;
  onClick: (e: ReactMouseEvent) => void;
  onPick: (range: boolean) => void;
  onMenu: (x: number, y: number) => void;
}) {
  const { n } = props;
  const folder = n.kind === "folder";
  return (
    <li
      data-row={n.id}
      onPointerDown={props.onPointerDown}
      onDragStart={(e) => e.preventDefault()}
      onClick={props.onClick}
      onContextMenu={(e) => {
        e.preventDefault();
        props.onMenu(e.clientX, e.clientY);
      }}
      className={`group relative flex cursor-default items-center gap-3 px-3 py-2 text-sm transition-colors duration-150 first:rounded-t-card last:rounded-b-card ${
        props.hint === "into" ? "bg-accent ring-2 ring-primary/50 ring-inset" : props.picked ? "bg-accent" : props.flash ? "bg-[var(--status-doing-bg)]" : "hover:bg-muted/60"
      } ${props.dragging ? "opacity-40" : ""}`}
    >
      {props.hint === "before" && <span className="absolute inset-x-2 -top-px h-0.5 rounded-full bg-primary" />}
      {props.hint === "after" && <span className="absolute inset-x-2 -bottom-px h-0.5 rounded-full bg-primary" />}
      <button
        type="button"
        role="checkbox"
        aria-checked={props.picked}
        aria-label={`选择「${n.title}」`}
        onClick={(e) => {
          e.stopPropagation();
          props.onPick(e.shiftKey);
        }}
        className={`grid size-4 shrink-0 place-items-center rounded-full border outline-none transition-opacity duration-150 focus-visible:ring-2 focus-visible:ring-ring ${
          props.picked ? "border-primary bg-primary text-primary-foreground" : `border-muted-foreground/50 ${props.picking ? "" : "opacity-0 group-hover:opacity-100 focus-visible:opacity-100"}`
        }`}
      >
        {props.picked && <Check size={11} strokeWidth={3} />}
      </button>
      {folder ? <Folder size={16} strokeWidth={1.75} className="shrink-0 text-muted-foreground" /> : <SiteIcon url={n.url} icons={props.icons} />}
      <span className="w-2/5 min-w-0 shrink-0 truncate font-medium">{n.title}</span>
      <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
        {folder ? `${linksIn(n)} 个网址` : n.url}
        {props.where && <span className="ml-2">· {props.where}</span>}
      </span>
      {n.onHome && <House size={13} aria-label="放在首页" className="shrink-0 text-primary" />}
      <button
        type="button"
        aria-label={`「${n.title}」的操作`}
        title="更多"
        onClick={(e) => {
          e.stopPropagation();
          const b = e.currentTarget.getBoundingClientRect();
          props.onMenu(b.left, b.bottom + 4);
        }}
        className="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 outline-none group-hover:opacity-100 hover:bg-muted hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"
      >
        <span className="block px-1 leading-none">⋯</span>
      </button>
    </li>
  );
}

function EditDialog(props: { editing: Editing; onClose: () => void }) {
  const mut = useBookmarkMutations();
  const e = props.editing;
  const node = e.mode === "edit" ? e.node : null;
  const kind = node ? node.kind : e.mode === "new" ? e.kind : "link";
  const [title, setTitle] = useState(node?.title ?? "");
  const [url, setUrl] = useState(node?.url ?? "");
  const [onHome, setOnHome] = useState(node?.onHome ?? false);
  const [error, setError] = useState<string | null>(null);
  const link = kind === "link";
  const submit = async (ev: FormEvent) => {
    ev.preventDefault();
    try {
      if (e.mode === "new") await mut.create.mutateAsync({ parentId: e.parentId, kind, title, url: link ? url : undefined, onHome: link && onHome });
      else await mut.update.mutateAsync({ id: e.node.id, patch: link ? { title, url, onHome } : { title } });
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal
      title={e.mode === "new" ? (link ? "新网址" : "新文件夹") : link ? "编辑网址" : "文件夹改名"}
      onClose={props.onClose}
      onSubmit={(ev) => void submit(ev)}
      error={error}
      submitLabel={e.mode === "new" ? "添加" : "保存"}
      submitDisabled={link ? !url.trim() : !title.trim()}
    >
      {link && (
        <Field label="网址">
          <TextInput autoFocus value={url} onChange={(v) => setUrl(v.target.value)} placeholder="https://" />
        </Field>
      )}
      <Field label="名称">
        <TextInput autoFocus={!link} value={title} onChange={(v) => setTitle(v.target.value)} maxLength={300} />
      </Field>
      {link && (
        <label className="flex items-center justify-between text-sm">
          放首页
          <Switch label="放首页" checked={onHome} onChange={setOnHome} />
        </label>
      )}
    </Modal>
  );
}
