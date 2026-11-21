// Left-column tags, built from full paths (#a/b/c; siblings in their dragged order, never-dragged ones
// after them in pinyin order): pinned first-level tags first (pin order, each with its sub-tags), then
// the rest of the tree. Each row: icon (click = change it), name, count, and a ⋯ menu — 置顶 (first
// level only) / 换图标 (the icon library) / 改名 / 删除 (rename and delete rewrite the notes; Rust moves the settings along).
// Drag a row (pointer events, like shared/sortable): onto the upper / lower part of a row = before /
// after it among its siblings (another parent = a move, i.e. a rename); onto its middle = into it, last.
// Pinned first-level tags reorder only among themselves. Fold state is a UI setting; the active tag's
// ancestors always show, and the filter follows a renamed / moved tag.
// Shared by 念念不忘 and 见字如面 (their tags don't mix): `scope` gives the module's own tag commands and wording.
import { createContext, useContext, useEffect, useRef, useState, type FormEvent, type PointerEvent as ReactPointerEvent } from "react";
import { createPortal } from "react-dom";
import { ChevronDown, Ellipsis } from "@/shared/icons";
import { IconDialog } from "@/shared/iconLibrary";
import { TagIcon } from "./tagIcons";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { isStringArray, useSetting } from "@/shared/settings";
import { InlineName } from "@/shared/InlineName";
import { Picked, Segmented } from "@/shared/ui";
import { useNoteMutations, type TagCount } from "./api";
import { Menu, type MenuAt } from "@/app/Menu";

/** A module's tag commands and words (念念不忘's by default). */
export type TagScope = {
  useMutations: () => Pick<ReturnType<typeof useNoteMutations>, "renameTag" | "setTag" | "placeTag" | "orderPins" | "deleteTag">;
  /** Settings key of the open branches. */
  openKey: string;
  color: string;
  /** 「条笔记」 / 「篇笔记」. */
  unit: string;
  /** Deleting a tag with its items: what happens to them. */
  withItems: { option: string; button: (n: number) => string; done: (n: number) => string; explain: (n: number) => string };
  /** More in a tag's 「⋯」 (见字如面: 导出…). */
  actions?: { value: string; label: string; run: (tag: string) => void }[];
};
export const NOTES_SCOPE: TagScope = {
  useMutations: useNoteMutations,
  openKey: "notes.tags_open",
  color: "var(--module-notes)",
  unit: "条笔记",
  withItems: { option: "连笔记一起删", button: (n) => `删除 ${n} 条笔记`, done: (n) => `已删除 ${n} 条笔记`, explain: (n) => `用到它和子标签的 ${n} 条笔记都会删掉。` },
};
const ScopeCtx = createContext<TagScope>(NOTES_SCOPE);

type Node = { path: string; name: string; count: number; icon: string | null; order: number | null; kids: Node[] };

const collator = new Intl.Collator("zh-Hans-CN-u-co-pinyin", { numeric: true });

export function buildTree(tags: TagCount[]): Node[] {
  const byPath = new Map<string, Node>();
  const roots: Node[] = [];
  for (const t of [...tags].sort((a, b) => a.tag.split("/").length - b.tag.split("/").length)) {
    const i = t.tag.lastIndexOf("/");
    const node: Node = { path: t.tag, name: t.tag.slice(i + 1), count: t.count, icon: t.icon, order: t.order, kids: [] };
    byPath.set(t.tag, node);
    (i < 0 ? roots : (byPath.get(t.tag.slice(0, i))?.kids ?? roots)).push(node);
  }
  const sort = (ns: Node[]) => {
    ns.sort((a, b) => (a.order ?? Infinity) - (b.order ?? Infinity) || collator.compare(a.name, b.name));
    ns.forEach((n) => sort(n.kids));
  };
  sort(roots);
  return roots;
}

const under = (t: string, root: string) => t === root || t.startsWith(`${root}/`);
const parentOf = (t: string) => (t.includes("/") ? t.slice(0, t.lastIndexOf("/")) : null);
const leafOf = (t: string) => t.slice(t.lastIndexOf("/") + 1);

function DeleteDialog(props: { tag: string; count: number; onClose: () => void; onDone: () => void }) {
  const [mode, setMode] = useState<"tag" | "notes">("tag");
  const [error, setError] = useState<string | null>(null);
  const scope = useContext(ScopeCtx);
  const m = scope.useMutations();
  const toast = useToast();
  const submit = (e: FormEvent) => {
    e.preventDefault();
    m.deleteTag.mutate(
      { tag: props.tag, notes: mode === "notes" },
      {
        onSuccess: ([n, kept]) => {
          toast(mode === "notes" ? scope.withItems.done(n) : `已从 ${n} ${scope.unit}里去掉${kept ? `，${kept} 条只有这个标签，留着没动` : ""}`);
          props.onDone();
          props.onClose();
        },
        onError: (err) => setError(errorText(err)),
      },
    );
  };
  return (
    <Modal title={`删除 #${props.tag}`} onClose={props.onClose} onSubmit={submit} error={error} submitLabel={mode === "notes" ? scope.withItems.button(props.count) : "去掉标签"}>
      <Segmented
        ariaLabel="删除方式"
        value={mode}
        onChange={setMode}
        options={[
          { value: "tag", label: "只去掉标签" },
          { value: "notes", label: scope.withItems.option },
        ]}
      />
      <p className="text-sm text-muted-foreground">
        {mode === "notes" ? scope.withItems.explain(props.count) : `${props.count} ${scope.unit}里的 #${props.tag} 和子标签去掉，笔记留着。`}
      </p>
    </Modal>
  );
}

function TagIconDialog(props: { tag: string; icon: string | null; onClose: () => void }) {
  const scope = useContext(ScopeCtx);
  const m = scope.useMutations();
  return <IconDialog title={`#${props.tag} 的图标`} icon={props.icon} color={scope.color} onPick={(icon) => m.setTag.mutate({ tag: props.tag, patch: { icon } })} onClose={props.onClose} />;
}

type Drop = { kind: "before" | "after" | "into"; path: string };
type Drag = { path: string; x: number; y: number; drop: Drop | null };
const THRESHOLD = 4;
const EDGE = 28;

function scrollParent(el: HTMLElement | null): HTMLElement | null {
  for (let e = el?.parentElement ?? null; e; e = e.parentElement) if (/(auto|scroll)/.test(getComputedStyle(e).overflowY)) return e;
  return null;
}

export function TagTree(props: { tags: TagCount[]; pinned: string[]; active: string | null; onSelect: (tag: string | null) => void; scope?: TagScope }) {
  const scope = props.scope ?? NOTES_SCOPE;
  return (
    <ScopeCtx.Provider value={scope}>
      <Tree {...props} scope={scope} />
    </ScopeCtx.Provider>
  );
}

function Tree(props: { tags: TagCount[]; pinned: string[]; active: string | null; onSelect: (tag: string | null) => void; scope: TagScope }) {
  const [open, setOpen] = useSetting<string[]>(props.scope.openKey, [], isStringArray);
  const [renaming, setRenaming] = useState<TagCount | null>(null);
  const [deleting, setDeleting] = useState<TagCount | null>(null);
  const [iconFor, setIconFor] = useState<TagCount | null>(null);
  const [menu, setMenu] = useState<MenuAt | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragged = useRef(false);
  const box = useRef<HTMLDivElement>(null);
  const m = props.scope.useMutations();
  const toast = useToast();
  const roots = buildTree(props.tags);
  const byPath = new Map(props.tags.map((t) => [t.tag, t]));
  const nodes = new Map<string, Node>();
  const walk = (ns: Node[]) => ns.forEach((n) => (nodes.set(n.path, n), walk(n.kids)));
  walk(roots);
  const isOpen = (n: Node) => open.includes(n.path) || (!!props.active && props.active.startsWith(`${n.path}/`));
  const toggle = (n: Node) => setOpen(open.includes(n.path) ? open.filter((p) => p !== n.path) : [...open, n.path]);
  // pinned first-level tags (with their sub-tags) go on top and aren't repeated in the tree
  const pinnedRoots = props.pinned.map((p) => roots.find((r) => r.path === p)).filter((n) => n !== undefined);
  const rest = roots.filter((r) => !props.pinned.includes(r.path));

  /** The filter follows a tag renamed or moved from `from` to `to` (null: gone). */
  const follow = (from: string, to: string | null) => {
    const a = props.active;
    if (a && under(a, from)) props.onSelect(to === null ? null : to + a.slice(from.length));
  };

  /** Where the pointer would drop the dragged tag, if anywhere. */
  const dropAt = (path: string, y: number): Drop | null => {
    const rows = [...(box.current?.querySelectorAll<HTMLElement>("[data-tag-path]") ?? [])];
    let el = rows.find((r) => {
      const b = r.getBoundingClientRect();
      return y >= b.top && y < b.bottom;
    });
    if (!el && rows.length) el = y < rows[0].getBoundingClientRect().top ? rows[0] : rows[rows.length - 1];
    if (!el) return null;
    const target = el.dataset.tagPath!;
    if (under(target, path)) return null;
    const b = el.getBoundingClientRect();
    const rel = (y - b.top) / b.height;
    const n = nodes.get(target);
    let kind: Drop["kind"] = rel < 0.3 ? "before" : rel > 0.7 ? "after" : "into";
    if (kind === "after" && n && n.kids.length && isOpen(n)) return { kind: "into", path: target };
    if (kind === "into") return target === parentOf(path) ? null : { kind, path: target };
    // first level: pinned ones only among themselves, the rest only among the rest
    if (!target.includes("/") && props.pinned.includes(target) !== props.pinned.includes(path)) kind = "into";
    if (kind === "into") return target === parentOf(path) ? null : { kind, path: target };
    return { kind, path: target };
  };

  const finish = (path: string, drop: Drop) => {
    let parent: string | null;
    let order: string[];
    let to: string;
    if (drop.kind === "into") {
      parent = drop.path;
      to = `${parent}/${leafOf(path)}`;
      order = [...(nodes.get(parent)?.kids ?? []).map((k) => k.path).filter((k) => k !== to), to];
      if (!open.includes(parent)) setOpen([...open, parent]);
    } else {
      parent = parentOf(drop.path);
      to = parent ? `${parent}/${leafOf(path)}` : leafOf(path);
      if (!parent && props.pinned.includes(path)) {
        const pins = props.pinned.filter((p) => p !== path);
        pins.splice(pins.indexOf(drop.path) + (drop.kind === "after" ? 1 : 0), 0, path);
        m.orderPins.mutate({ order: pins });
        return;
      }
      const siblings = (parent ? (nodes.get(parent)?.kids ?? []) : rest).map((k) => k.path).filter((k) => k !== path && k !== to);
      const at = drop.path === to ? siblings.length : siblings.indexOf(drop.path) + (drop.kind === "after" ? 1 : 0);
      siblings.splice(Math.max(0, at), 0, to);
      order = siblings;
    }
    m.placeTag.mutate(
      { tag: path, parent, order },
      {
        onSuccess: (moved) => {
          if (moved > 0) toast(`已移到 #${to}，改了 ${moved} ${props.scope.unit}`);
          follow(path, to);
        },
      },
    );
  };

  // auto-scroll the column while dragging near its edges
  useEffect(() => {
    if (!drag) return;
    const sc = scrollParent(box.current);
    if (!sc) return;
    const id = window.setInterval(() => {
      const b = sc.getBoundingClientRect();
      const step = drag.y < b.top + EDGE ? -8 : drag.y > b.bottom - EDGE ? 8 : 0;
      if (step) sc.scrollTop += step;
    }, 16);
    return () => window.clearInterval(id);
  }, [drag]);

  const onPointerDown = (path: string) => (e: ReactPointerEvent<HTMLElement>) => {
    dragged.current = false;
    if (e.button !== 0 || (e.target as HTMLElement).closest("[aria-haspopup], [aria-expanded]")) return;
    const sx = e.clientX;
    const sy = e.clientY;
    let current: Drag | null = null;
    const move = (ev: PointerEvent) => {
      if (!current && Math.hypot(ev.clientX - sx, ev.clientY - sy) < THRESHOLD) return;
      if (!current) window.getSelection()?.removeAllRanges();
      current = { path, x: ev.clientX, y: ev.clientY, drop: dropAt(path, ev.clientY) };
      setDrag(current);
    };
    const up = () => {
      stop();
      if (!current) return;
      dragged.current = true;
      setDrag(null);
      if (current.drop) finish(path, current.drop);
    };
    const key = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      ev.stopPropagation();
      stop();
      if (current) dragged.current = true;
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

  /** A tag's actions: the 「⋯」 and right-click offer the same. */
  const actionsOf = (t: TagCount) => {
    const pinned = props.pinned.includes(t.tag);
    return [
      ...(!t.tag.includes("/") ? [{ value: "pin", label: pinned ? "取消置顶" : "置顶" }] : []),
      { value: "icon", label: "换图标…" },
      { value: "rename", label: "改名" },
      { value: "delete", label: "删除…" },
      ...(props.scope.actions ?? []).map((x) => ({ value: x.value, label: x.label })),
    ];
  };
  const runAction = (t: TagCount, a: string | null) => {
    if (a === "pin") m.setTag.mutate({ tag: t.tag, patch: { pinned: !props.pinned.includes(t.tag) } });
    if (a === "icon") setIconFor(t);
    if (a === "rename") setRenaming(t);
    if (a === "delete") setDeleting(t);
    props.scope.actions?.find((x) => x.value === a)?.run(t.tag);
  };

  const row = (t: TagCount, label: string, depth: number, kids?: { expanded: boolean; onToggle: () => void }) => {
    const on = props.active === t.tag;
    const into = drag?.drop?.kind === "into" && drag.drop.path === t.tag;
    return (
      <div
        data-tag-path={t.tag}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY, rows: actionsOf(t).map((a) => ({ label: a.label, onSelect: () => runAction(t, a.value) })) });
        }}
        onPointerDown={onPointerDown(t.tag)}
        onDragStart={(e) => e.preventDefault()}
        onClickCapture={(e) => {
          if (!dragged.current) return;
          dragged.current = false;
          e.stopPropagation();
          e.preventDefault();
        }}
        className={`group/tag side-row !min-h-0 ${into ? "bg-accent ring-2 ring-primary/50" : on ? "on" : ""} ${drag?.path === t.tag ? "opacity-40" : ""}`}
        style={{ paddingLeft: 6 + depth * 14 }}
      >
        {on && !into && <Picked seed={1426} />}
        <button
          type="button"
          aria-label={`换「${t.tag}」的图标`}
          onClick={() => setIconFor(t)}
          className="grid size-5 shrink-0 place-items-center rounded outline-none hover:bg-black/5 focus-visible:ring-2 focus-visible:ring-ring dark:hover:bg-white/10"
        >
          <TagIcon icon={t.icon} />
        </button>
        {renaming?.tag === t.tag ? (
          <div className="min-w-0 flex-1 py-1 pl-1">
            <InlineName
              initial={t.tag}
              label={`「${t.tag}」改名`}
              max={60}
              onDone={(to) => {
                setRenaming(null);
                if (to === null) return;
                m.renameTag.mutate(
                  { from: t.tag, to },
                  {
                    onSuccess: (n) => {
                      if (n > 0) toast(`已改到 ${n} ${props.scope.unit}`);
                      follow(t.tag, to.replace(/^#/, "").replace(/\/+$/, ""));
                    },
                    onError: (err) => toast(errorText(err), "error"),
                  },
                );
              }}
            />
          </div>
        ) : (
          <button
            type="button"
            aria-current={on}
            onClick={() => props.onSelect(t.tag)}
            className="flex min-w-0 flex-1 items-center gap-1.5 rounded-control py-1.5 pr-1 pl-2 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <span className="min-w-0 flex-1 truncate">{label}</span>
            <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{t.count}</span>
          </button>
        )}
        <Dropdown<string | null>
          plain
          ariaLabel={`「${t.tag}」的更多操作`}
          value={null}
          onChange={(a) => runAction(t, a)}
          align="end"
          options={actionsOf(t).map((x) => ({ value: x.value as string | null, label: x.label }))}
          renderTrigger={() => <Ellipsis size={13} />}
          triggerClassName="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 outline-none group-hover/tag:opacity-100 hover:bg-muted hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring aria-expanded:opacity-100"
        />
        <span className="mr-1.5 flex w-[18px] shrink-0 justify-center">
          {kids && (
            <button
              type="button"
              aria-label={kids.expanded ? `收起「${t.tag}」` : `展开「${t.tag}」`}
              aria-expanded={kids.expanded}
              onClick={kids.onToggle}
              className="rounded p-0.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
            >
              <ChevronDown size={13} className={`transition-transform duration-150 ${kids.expanded ? "" : "-rotate-90"}`} />
            </button>
          )}
        </span>
      </div>
    );
  };

  const node = (n: Node, depth: number) => {
    const expanded = isOpen(n);
    return (
      <div key={n.path}>
        {row(byPath.get(n.path) ?? { tag: n.path, count: n.count, icon: n.icon, order: n.order }, n.name, depth, n.kids.length > 0 ? { expanded, onToggle: () => toggle(n) } : undefined)}
        {expanded && n.kids.map((k) => node(k, depth + 1))}
      </div>
    );
  };

  // the line where a before / after drop lands, indented to the target's level
  const line = (() => {
    const d = drag?.drop;
    if (!d || d.kind === "into") return null;
    const el = box.current?.querySelector<HTMLElement>(`[data-tag-path="${CSS.escape(d.path)}"]`);
    if (!el) return null;
    const b = el.getBoundingClientRect();
    const indent = 6 + (d.path.split("/").length - 1) * 14;
    return { top: (d.kind === "before" ? b.top : b.bottom) - 1, left: b.left + indent, width: b.width - indent };
  })();
  return (
    <div ref={box} className={drag ? "cursor-grabbing select-none" : undefined}>
      {pinnedRoots.length > 0 && (
        <>
          <div className="px-2.5 pt-4 pb-1 text-xs text-muted-foreground">置顶</div>
          <div className="space-y-0.5">{pinnedRoots.map((n) => node(n, 0))}</div>
        </>
      )}
      {rest.length > 0 && (
        <>
          {pinnedRoots.length > 0 ? <div className="px-2.5 pt-4 pb-1 text-xs text-muted-foreground">标签</div> : <div className="side-rule" />}
          <div className="space-y-0.5">{rest.map((n) => node(n, 0))}</div>
        </>
      )}
      {deleting && <DeleteDialog tag={deleting.tag} count={deleting.count} onClose={() => setDeleting(null)} onDone={() => follow(deleting.tag, null)} />}
      {iconFor && <TagIconDialog tag={iconFor.tag} icon={iconFor.icon} onClose={() => setIconFor(null)} />}
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
      {drag &&
        createPortal(
          <>
            {line && <div className="pointer-events-none fixed z-50 h-0.5 rounded-full bg-primary" style={line} />}
            <div className="pointer-events-none fixed z-50 max-w-48 truncate rounded-control border bg-popover px-2 py-0.5 text-sm shadow-md" style={{ left: drag.x + 12, top: drag.y + 10 }}>
              #{leafOf(drag.path)}
            </div>
          </>,
          document.body,
        )}
    </div>
  );
}
