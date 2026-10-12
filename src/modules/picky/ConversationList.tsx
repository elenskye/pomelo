// The conversation list (rules from the old Picky, enforced in Rust library.rs):
// 置顶 · 项目 (each collapsible, with its conversations) · 最近 · 归档 (folded). Each list follows last
// activity until dragged by hand. ⋯ on a row: 改名 / 置顶 / 复制整段对话 / 导出 / 移到项目 / 归档 / 删除.
// ⌘-click / ⇧-click select several (a bar at the bottom acts on them; no dragging meanwhile). Drag a row:
// onto another row = that place in its list; onto a list or project header = its top. Into a project
// moves it in; into 最近 takes it out of its project and unpins it; into 置顶 pins it (keeps its project).
// Project headers drag to reorder projects. Esc or a drop outside cancels. Everything here can be undone.
import { useEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ChevronRight, Ellipsis, Folder, Library as LibraryIcon, Plus, Search, Settings2, X } from "lucide-react";
import { Dropdown, type DropdownAction, type DropdownOption } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { IconButton } from "@/shared/ui";
import { LIBRARY_KEY, isBool, isNumbers, pickyApi, usePickyUi, type Library, type ListKey, type ProjectRow, type SessionRow } from "./api";
import { useConversationActions } from "./actions";
import { ProjectDialog } from "./ProjectDialog";

type Target = { list: ListKey; before?: number; after?: number; header?: boolean };
type Drag = { kind: "session" | "project"; id: number; title: string; x: number; y: number; target: Target | null };

const THRESHOLD = 5;
const EDGE = 28;

/** Pointer drags across the list's groups (HTML drag-and-drop is unreliable in the macOS webview). */
function useListDrag(box: RefObject<HTMLElement | null>, onDrop: (d: Drag) => void) {
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragged = useRef(false);
  const drop = useRef(onDrop);
  drop.current = onDrop;

  const begin = (e: React.PointerEvent, kind: Drag["kind"], id: number, title: string) => {
    if (e.button !== 0 || e.metaKey || e.shiftKey || e.ctrlKey) return;
    dragged.current = false;
    const sx = e.clientX;
    const sy = e.clientY;
    let cur: Drag | null = null;
    let lastY = sy;
    let timer = 0;
    const find = (x: number, y: number): Target | null => {
      const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-drop]");
      if (!el || !box.current?.contains(el)) return null;
      const r = el.getBoundingClientRect();
      const upper = y < r.top + r.height / 2;
      if (kind === "project") {
        const pid = Number(el.dataset.project);
        if (el.dataset.drop !== "project" || pid === id) return null;
        return upper ? { list: "projects", before: pid } : { list: "projects", after: pid };
      }
      if (el.dataset.drop === "session") {
        const sid = Number(el.dataset.id);
        if (sid === id) return null;
        return upper ? { list: el.dataset.list!, before: sid } : { list: el.dataset.list!, after: sid };
      }
      return { list: el.dataset.list!, header: true };
    };
    const scroll = () => {
      const el = box.current;
      if (!el || !cur) return;
      const r = el.getBoundingClientRect();
      const d = lastY < r.top + EDGE ? -(r.top + EDGE - lastY) : lastY > r.bottom - EDGE ? lastY - (r.bottom - EDGE) : 0;
      if (d) {
        el.scrollTop += Math.sign(d) * Math.min(18, 3 + Math.abs(d) / 2); // faster nearer the edge
        cur = { ...cur, target: find(cur.x, cur.y) };
        setDrag(cur);
      }
    };
    const move = (ev: PointerEvent) => {
      if (!cur) {
        if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < THRESHOLD) return;
        window.getSelection()?.removeAllRanges();
        cur = { kind, id, title, x: ev.clientX, y: ev.clientY, target: null };
        timer = window.setInterval(scroll, 30);
      }
      lastY = ev.clientY;
      cur = { ...cur, x: ev.clientX, y: ev.clientY, target: find(ev.clientX, ev.clientY) };
      setDrag(cur);
    };
    const stop = () => {
      clearInterval(timer);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key, true);
    };
    const up = () => {
      stop();
      if (!cur) return;
      dragged.current = true;
      setTimeout(() => (dragged.current = false)); // only the click that ends this drag is swallowed
      setDrag(null);
      if (cur.target) drop.current(cur);
    };
    const cancel = () => {
      stop();
      if (cur) dragged.current = true;
      setDrag(null);
    };
    const key = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape" || !cur) return;
      ev.preventDefault();
      ev.stopPropagation();
      cancel();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", key, true);
  };
  /** The click that ends a drag must not open the row. */
  const swallow = () => {
    if (!dragged.current) return false;
    dragged.current = false;
    return true;
  };
  return { drag, begin, swallow };
}

const menuTrigger =
  "rounded-control p-0.5 text-muted-foreground opacity-0 outline-none group-hover:opacity-100 hover:bg-muted hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring aria-expanded:opacity-100";

function RenameInput(props: { initial: string; onDone: (title: string | null) => void }) {
  const [v, setV] = useState(props.initial);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => ref.current?.select(), []);
  return (
    <input
      ref={ref}
      value={v}
      aria-label="对话名称"
      maxLength={120}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => props.onDone(v)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && !e.nativeEvent.isComposing) props.onDone(v);
        else if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          props.onDone(null);
        }
      }}
      className="w-full min-w-0 rounded-control border bg-card px-2 py-1 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
    />
  );
}

export function ConversationList(props: {
  library: Library;
  current: number | null;
  /** The conversation a reply is being written in. */
  generating: number | null;
  onOpen: (id: number) => void;
  onNewInProject: (projectId: number) => void;
  onSearch: () => void;
  onManage: () => void;
  onProjectFiles: (projectId: number) => void;
}) {
  const lib = props.library;
  const qc = useQueryClient();
  const toast = useToast();
  const act = useConversationActions();
  const box = useRef<HTMLDivElement>(null);
  const [collapsed, setCollapsed] = usePickyUi("ui.collapsed_projects", [] as number[], isNumbers);
  const [archiveOpen, setArchiveOpen] = usePickyUi("ui.archive_open", false, isBool);
  const [selected, setSelected] = useState<number[]>([]);
  const anchor = useRef<number | null>(null);
  const [renaming, setRenaming] = useState<number | null>(null);
  const [projectDialog, setProjectDialog] = useState<ProjectRow | "new" | null>(null);

  const all = [...lib.pinned, ...lib.projects.flatMap((p) => p.sessions), ...lib.recent, ...lib.archived];
  const byId = new Map(all.map((s) => [s.id, s]));
  const order = [...lib.pinned, ...lib.projects.flatMap((p) => (collapsed.includes(p.id) ? [] : p.sessions)), ...lib.recent, ...(archiveOpen ? lib.archived : [])].map((s) => s.id);
  // selection only holds rows that still exist
  const sel = selected.filter((id) => byId.has(id));

  const refresh = () => qc.invalidateQueries({ queryKey: LIBRARY_KEY });
  const { drag, begin, swallow } = useListDrag(box, (d) => {
    const t = d.target!;
    if (d.kind === "project") {
      const ids = lib.projects.map((p) => p.id).filter((id) => id !== d.id);
      const ref = (t.before ?? t.after)!;
      ids.splice(ids.indexOf(ref) + (t.after !== undefined ? 1 : 0), 0, d.id);
      void pickyApi.projectsReorder(ids).then(refresh, (e) => toast(errorText(e), "error"));
    } else {
      void pickyApi.drop(d.id, t.list, t.before ?? null, t.after ?? null).then(refresh, (e) => toast(errorText(e), "error"));
    }
  });
  const dragging = drag?.kind === "session";

  useEffect(() => {
    if (!sel.length) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setSelected([]);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [sel.length]);

  const click = (e: React.MouseEvent, s: SessionRow) => {
    if (swallow()) return;
    if (e.metaKey) {
      setSelected(sel.includes(s.id) ? sel.filter((x) => x !== s.id) : [...sel, s.id]);
      anchor.current = s.id;
    } else if (e.shiftKey) {
      const from = order.indexOf(anchor.current ?? props.current ?? s.id);
      const to = order.indexOf(s.id);
      if (from < 0 || to < 0) return;
      setSelected(order.slice(Math.min(from, to), Math.max(from, to) + 1));
    } else {
      setSelected([]);
      anchor.current = s.id;
      props.onOpen(s.id);
    }
  };

  const sessionMenu = (s: SessionRow): [DropdownOption<string>[], DropdownAction[]] => [
    [
      { value: "rename", label: "改名" },
      { value: s.pinned ? "unpin" : "pin", label: s.pinned ? "取消置顶" : "置顶" },
      { value: "copy", label: "复制整段对话" },
      { value: "md", label: "导出 Markdown…" },
      { value: "pkg", label: "导出完整包…" },
      ...lib.projects.filter((p) => p.id !== s.projectId).map((p) => ({ value: `move:${p.id}`, label: p.name, group: "移到项目" })),
      ...(s.projectId != null ? [{ value: "unmove", label: "移出项目", group: "移到项目" }] : []),
    ],
    [
      { label: <span className="text-foreground">{s.archived ? "恢复" : "归档"}</span>, onSelect: () => void act.archive([s.id], !s.archived) },
      { label: <span className="text-destructive">删除</span>, onSelect: () => void act.remove([s.id], s.title) },
    ],
  ];
  const onMenu = (s: SessionRow, v: string) => {
    if (v === "rename") setRenaming(s.id);
    else if (v === "pin" || v === "unpin") void act.pin([s.id], v === "pin");
    else if (v === "copy") void act.copy(s.id);
    else if (v === "md") void act.exportMarkdown(s.id, s.title);
    else if (v === "pkg") void act.exportPackage(s.id, s.title);
    else if (v === "unmove") void act.move([s.id], null);
    else if (v.startsWith("move:")) void act.move([s.id], Number(v.slice(5)));
  };

  const row = (s: SessionRow, list: ListKey) => {
    const line = dragging && drag.target && !drag.target.header && drag.target.list === list ? (drag.target.before === s.id ? "top" : drag.target.after === s.id ? "bottom" : null) : null;
    const [options, actions] = sessionMenu(s);
    const isSel = sel.includes(s.id);
    return (
      <div
        key={s.id}
        data-drop="session"
        data-id={s.id}
        data-list={list}
        onPointerDown={(e) => !sel.length && renaming !== s.id && begin(e, "session", s.id, s.title)}
        className={`group relative flex items-center gap-1 rounded-control pr-1 transition-colors duration-150 ${
          isSel ? "bg-accent ring-1 ring-primary/40" : s.id === props.current ? "bg-accent" : "hover:bg-muted/70"
        } ${drag?.kind === "session" && drag.id === s.id ? "opacity-40" : ""}`}
      >
        {line && <span className={`pointer-events-none absolute right-1 left-1 h-0.5 rounded bg-primary ${line === "top" ? "-top-px" : "-bottom-px"}`} />}
        {renaming === s.id ? (
          <div className="flex-1 py-0.5 pl-1">
            <RenameInput
              initial={s.title}
              onDone={(t) => {
                setRenaming(null);
                if (t !== null && t !== s.title) void act.rename(s.id, t);
              }}
            />
          </div>
        ) : (
          <button
            type="button"
            aria-current={s.id === props.current}
            title={s.title}
            onClick={(e) => click(e, s)}
            className="min-w-0 flex-1 truncate py-1.5 pl-2.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {s.title}
          </button>
        )}
        {props.generating === s.id && <span className="size-1.5 shrink-0 animate-pulse rounded-full" style={{ background: "var(--status-doing-fg)" }} aria-label="正在写回复" />}
        {renaming !== s.id && (
          <Dropdown<string | null>
            plain
            ariaLabel="更多"
            value={null}
            align="end"
            options={options}
            actions={actions}
            onChange={(v) => v && onMenu(s, v)}
            renderTrigger={() => <Ellipsis size={14} />}
            triggerClassName={menuTrigger}
          />
        )}
      </div>
    );
  };

  /** A list's label row; it is also a drop target (= the top of that list). */
  const label = (text: ReactNode, list: ListKey | null, extra?: ReactNode, onClick?: () => void) => {
    const hot = dragging && list && drag.target?.header && drag.target.list === list;
    return (
      <div
        data-drop={list ? "list" : undefined}
        data-list={list ?? undefined}
        onClick={onClick}
        className={`mt-3 flex items-center gap-1 rounded-control px-2.5 py-0.5 text-xs text-muted-foreground first:mt-0 ${hot ? "bg-accent ring-1 ring-primary/40" : ""} ${onClick ? "cursor-default hover:text-foreground" : ""}`}
      >
        <span className="flex flex-1 items-center gap-1">{text}</span>
        {extra}
      </div>
    );
  };

  const projectHeader = (p: ProjectRow) => {
    const list = `project:${p.id}`;
    const isCollapsed = collapsed.includes(p.id);
    const hot = dragging && drag.target?.header && drag.target.list === list;
    const line = drag?.kind === "project" && drag.target ? (drag.target.before === p.id ? "top" : drag.target.after === p.id ? "bottom" : null) : null;
    return (
      <div
        data-drop="project"
        data-project={p.id}
        data-list={list}
        onPointerDown={(e) => !sel.length && begin(e, "project", p.id, p.name)}
        className={`group relative flex items-center gap-1 rounded-control pr-1 transition-colors duration-150 ${hot ? "bg-accent ring-1 ring-primary/40" : "hover:bg-muted/70"} ${
          drag?.kind === "project" && drag.id === p.id ? "opacity-40" : ""
        }`}
      >
        {line && <span className={`pointer-events-none absolute right-1 left-1 h-0.5 rounded bg-primary ${line === "top" ? "-top-px" : "-bottom-px"}`} />}
        <button
          type="button"
          aria-expanded={!isCollapsed}
          onClick={() => !swallow() && setCollapsed(isCollapsed ? collapsed.filter((x) => x !== p.id) : [...collapsed, p.id])}
          className="flex min-w-0 flex-1 items-center gap-1.5 py-1.5 pl-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <ChevronRight size={12} className={`shrink-0 text-muted-foreground transition-transform duration-150 ${isCollapsed ? "" : "rotate-90"}`} />
          <Folder size={14} strokeWidth={1.5} className="shrink-0 text-muted-foreground" />
          <span className="truncate">{p.name}</span>
        </button>
        <Dropdown<string | null>
          plain
          ariaLabel="项目菜单"
          value={null}
          align="end"
          options={[
            { value: "new", label: "新对话" },
            { value: "files", label: "资料…" },
            { value: "edit", label: "项目设置…" },
          ]}
          actions={[{ label: <span className="text-destructive">删除项目</span>, onSelect: () => void removeProject(p) }]}
          onChange={(v) => (v === "new" ? props.onNewInProject(p.id) : v === "edit" ? setProjectDialog(p) : v === "files" ? props.onProjectFiles(p.id) : null)}
          renderTrigger={() => <Ellipsis size={14} />}
          triggerClassName={menuTrigger}
        />
      </div>
    );
  };

  const removeProject = async (p: ProjectRow) => {
    if (await act.confirm({ title: `删除项目「${p.name}」？`, message: "里面的对话会留下，移到「最近」。", confirmLabel: "删除", danger: true }))
      void pickyApi.projectDelete(p.id).then(refresh, (e) => toast(errorText(e), "error"));
  };

  const allPinned = sel.length > 0 && sel.every((id) => byId.get(id)?.pinned);
  const allArchived = sel.length > 0 && sel.every((id) => byId.get(id)?.archived);
  const batchMove: DropdownOption<string>[] = [...lib.projects.map((p) => ({ value: String(p.id), label: p.name, group: "移到项目" })), { value: "none", label: "移出项目", group: "移到项目" }];

  return (
    <div className="flex w-[248px] shrink-0 flex-col">
      <div className="flex items-center pr-2 pb-1">
        <button
          type="button"
          onClick={props.onSearch}
          className="flex flex-1 items-center gap-1.5 rounded-control px-2.5 py-1.5 text-sm text-muted-foreground outline-none hover:bg-muted/70 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Search size={14} /> 搜索对话
        </button>
      </div>
      <div ref={box} className="-m-1 min-h-0 flex-1 overflow-y-auto p-1 pr-2">
        {(lib.pinned.length > 0 || dragging) && label("置顶", "pinned")}
        {lib.pinned.map((s) => row(s, "pinned"))}

        {label(
          "项目",
          null,
          <IconButton label="新建项目" onClick={() => setProjectDialog("new")}>
            <Plus size={13} strokeWidth={1.75} />
          </IconButton>,
        )}
        {lib.projects.map((p) => (
          <div key={p.id}>
            {projectHeader(p)}
            {!collapsed.includes(p.id) && (
              <div className="pl-4">
                {p.files > 0 && (
                  <button
                    type="button"
                    onClick={() => props.onProjectFiles(p.id)}
                    className="flex w-full items-center gap-1.5 rounded-control px-2.5 py-1 text-left text-sm text-muted-foreground outline-none hover:bg-muted/70 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    <LibraryIcon size={13} strokeWidth={1.5} /> 资料 <span className="text-xs">{p.files}</span>
                  </button>
                )}
                {p.sessions.map((s) => row(s, `project:${p.id}`))}
              </div>
            )}
          </div>
        ))}

        {(lib.recent.length > 0 || dragging) && label("最近", "recent")}
        {lib.recent.map((s) => row(s, "recent"))}

        {(lib.archived.length > 0 || dragging) &&
          label(
            <>
              <ChevronRight size={12} className={`transition-transform duration-150 ${archiveOpen ? "rotate-90" : ""}`} />
              归档 {lib.archived.length || ""}
            </>,
            "archived",
            null,
            () => setArchiveOpen(!archiveOpen),
          )}
        {archiveOpen && lib.archived.map((s) => row(s, "archived"))}
      </div>

      {sel.length > 0 && (
        <div className="mt-2 mr-2 flex flex-wrap items-center gap-1 rounded-card border bg-card p-1.5 text-sm shadow-sm">
          <span className="px-1.5 text-muted-foreground">已选 {sel.length}</span>
          <button type="button" className="rounded-control px-2 py-1 hover:bg-muted" onClick={() => void act.pin(sel, !allPinned)}>
            {allPinned ? "取消置顶" : "置顶"}
          </button>
          <Dropdown<string | null>
            plain
            ariaLabel="移到项目"
            value={null}
            options={batchMove}
            onChange={(v) => v && void act.move(sel, v === "none" ? null : Number(v))}
            renderTrigger={() => <span>移到</span>}
            triggerClassName="rounded-control px-2 py-1 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
          />
          <button type="button" className="rounded-control px-2 py-1 hover:bg-muted" onClick={() => void act.archive(sel, !allArchived)}>
            {allArchived ? "恢复" : "归档"}
          </button>
          <button type="button" className="rounded-control px-2 py-1 text-destructive hover:bg-muted" onClick={() => void act.remove(sel).then((ok) => ok && setSelected([]))}>
            删除
          </button>
          <IconButton label="取消选择" onClick={() => setSelected([])}>
            <X size={13} />
          </IconButton>
        </div>
      )}

      <button
        type="button"
        onClick={props.onManage}
        className="mt-1 mr-2 flex items-center gap-1.5 rounded-control px-2.5 py-1.5 text-sm text-muted-foreground outline-none hover:bg-muted/70 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Settings2 size={14} strokeWidth={1.5} /> 管理
      </button>

      {drag && (
        <div style={{ position: "fixed", left: drag.x + 12, top: drag.y + 6 }} className="pointer-events-none z-50 max-w-56 truncate rounded-control border bg-popover px-2.5 py-1 text-sm shadow-lg">
          {drag.title}
        </div>
      )}
      {projectDialog && <ProjectDialog project={projectDialog === "new" ? null : projectDialog} onClose={() => setProjectDialog(null)} />}
      {act.confirmDialog}
    </div>
  );
}

