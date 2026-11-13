// 有板有眼: visual-atelier's canvases inside Pomelo (Dawn Plan · Phase 8). The canvases are shown by that repository's
// own viewer, the bench — a local server whose page is the canvas UI. Here: 启动 / 停止 it (Rust, canvas.rs), a
// side column, and the bench's UI in a frame beside it — pan and zoom, pages, one board full size, Tweaks, 批准 all
// happen in there, nothing opens a browser.
// The side column is the bench's four lists, each folds (站主 2026-11-13; which are folded: setting `canvas.closed`):
// Design System, Motion Deck, Explore Deck, Questionnaire. The Explore Deck being looked at lists its pages, each
// folds too (the page being looked at is open), and under an open page its boards.
// The column and the frame follow each other: a click here moves the frame (its address is replaced, no history
// step); the bench tells this page where it went (window message `{ source: "dw", type: "route" }`, with `ds` or
// `motion` when it shows a Design System or a Motion Deck).
// 改名、删除（站主 2026-11-13）：Explore Deck、问卷页和一块画布的每一页，右键或行尾的「⋯」。做的是 visual-atelier 的 bench
// （canvas.rs → 它的接口），删了的进它的回收处；左栏顶上一条「撤销」放回去。bench 自己的界面里也能改，改了它发
// `{ source: "dw", type: "changed" }`，这边重新读清单。
// Nothing is kept except the last thing looked at (setting `canvas.last`), the folded lists and, when changed, the folder.
import { useEffect, useRef, useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ChevronRight, Ellipsis, FolderOpen, Pencil, Play, RotateCcw, Square, Trash2 } from "@/shared/icons";
import { moduleById } from "@/app/modules";
import { Menu, type MenuAt } from "@/app/Menu";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { isStringArray, useSetting } from "@/shared/settings";
import { Button, EmptyState, Field, PageHeader, Picked, RefreshButton, TextInput } from "@/shared/ui";
import { addressOf, CANVAS_KEY, canvasApi, OVERVIEW_KEY, useCanvasBoards, useCanvasList, useCanvasOverview, type CanvasRow, type Kind, type Overview, type Page, type Section } from "./api";
import "./canvas.css";

type At = { kind: Kind; name: string; page: string | null; board: string | null };
const KINDS: Kind[] = ["ds", "motion", "canvas", "page"];
/** `canvas.last` from before the four lists has no kind: it was a canvas or a page, found by name. */
const isAt = (v: unknown): v is (Omit<At, "kind"> & { kind?: Kind }) | null =>
  v === null || (!!v && typeof (v as At).name === "string" && ((v as At).kind === undefined || KINDS.includes((v as At).kind)));
const same = (a: { kind: Kind; name: string } | null | undefined, b: { kind: Kind; name: string } | null | undefined) => !!a && !!b && a.kind === b.kind && a.name === b.name;

function Stopped(props: { o: Overview; busy: boolean; onStart: () => void; onFolder: () => void; onDefault: () => void }) {
  const { o } = props;
  const why = o.problem ?? o.error;
  return (
    <EmptyState
      title={why ? "画布服务起不来" : "画布服务没在跑"}
      hint={
        <span className="block max-w-xl space-y-1">
          {why && <span className="selectable block whitespace-pre-wrap text-destructive">{why}</span>}
          <span className="selectable block">{o.folder}</span>
        </span>
      }
      action={
        <div className="flex items-center gap-2">
          <Button variant="ghost" onClick={props.onFolder}>
            <FolderOpen size={14} /> 换文件夹…
          </Button>
          {!o.isDefault && (
            <Button variant="ghost" onClick={props.onDefault}>
              用回默认的
            </Button>
          )}
          <Button variant="primary" disabled={props.busy || !!o.problem} onClick={props.onStart}>
            <Play size={14} /> {props.busy || o.state === "starting" ? "正在启动…" : "启动"}
          </Button>
        </div>
      }
    />
  );
}

/** 改名：一个名字的框，回车或「改名」交出去，没改成就在框下面说为什么。 */
function RenameDialog(props: { title: string; initial: string; hint?: string; onSubmit: (name: string) => Promise<unknown>; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [name, setName] = useState(props.initial);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const save = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim() || name.trim() === props.initial) return props.onClose();
    setError(null);
    try {
      await props.onSubmit(name.trim());
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <dialog
      ref={ref}
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className="m-auto h-fit w-[420px] rounded-card border bg-popover p-5 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <form onSubmit={save} className="space-y-4">
        <p className="text-base font-bold">{props.title}</p>
        <Field label="名字">
          <TextInput autoFocus value={name} maxLength={80} onChange={(e) => setName(e.target.value)} onFocus={(e) => e.target.select()} />
        </Field>
        {props.hint && <p className="text-sm text-muted-foreground">{props.hint}</p>}
        {error && <p className="selectable text-sm text-destructive">{error}</p>}
        <div className="flex justify-end gap-2">
          <Button type="button" onClick={props.onClose}>
            取消
          </Button>
          <Button type="submit" variant="primary">
            改名
          </Button>
        </div>
      </form>
    </dialog>
  );
}

/** 行尾的「⋯」：指着这一行才露出来。 */
function MoreButton(props: { label: string; onOpen: (el: HTMLElement) => void }) {
  return (
    <button
      type="button"
      aria-label={`${props.label}：改名、删除`}
      title="改名、删除"
      onClick={(e) => {
        e.stopPropagation();
        props.onOpen(e.currentTarget);
      }}
      className="cv-more"
    >
      <Ellipsis size={14} />
    </button>
  );
}

/** The little arrow in front of something that folds. */
function Fold(props: { open: boolean; label: string; onToggle: () => void }) {
  return (
    <button type="button" aria-expanded={props.open} aria-label={`${props.open ? "收起" : "展开"}${props.label}`} onClick={props.onToggle} className="cv-fold">
      <ChevronRight size={13} className={`transition-transform duration-150 ${props.open ? "rotate-90" : ""}`} />
    </button>
  );
}

export default function CanvasPage() {
  const m = moduleById("canvas");
  const qc = useQueryClient();
  const toast = useToast();
  const { data: o } = useCanvasOverview();
  const running = o?.state === "running";
  const { data: sections = [] } = useCanvasList(running);
  const rows = sections.flatMap((s) => s.items);
  const [lastSaved, setLast] = useSetting("canvas.last", null, isAt);
  const last: At | null = lastSaved ? { ...lastSaved, kind: lastSaved.kind ?? rows.find((r) => r.name === lastSaved.name && (r.kind === "canvas" || r.kind === "page"))?.kind ?? "canvas" } : null;
  const [closed, setClosed] = useSetting<string[]>("canvas.closed", [], isStringArray);
  const [at, setAt] = useState<At | null>(null);
  /** Pages of the open Explore Deck shown unfolded (the one being looked at always is). */
  const [openPages, setOpenPages] = useState<string[]>([]);
  /** The open Explore Deck's pages folded away under it. */
  const [treeShut, setTreeShut] = useState(false);
  const [busy, setBusy] = useState(false);
  const [menu, setMenu] = useState<MenuAt | null>(null);
  const [renaming, setRenaming] = useState<{ row: CanvasRow; page: Page | null } | null>(null);
  /** The last delete, while it can still be undone from here (the bench's trash keeps it longer). */
  const [undoable, setUndoable] = useState<{ id: string; label: string } | null>(null);
  const undoTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const [confirm, confirmDialog] = useConfirm();
  const frame = useRef<HTMLIFrameElement>(null);
  /** The address the frame was given first; later moves replace its location instead (no reload, no history step). */
  const [src, setSrc] = useState<string | null>(null);
  const rowsRef = useRef(rows);
  rowsRef.current = rows;

  const current = rows.find((r) => same(r, at));
  const { data: detail } = useCanvasBoards(running && current?.kind === "canvas", current?.kind === "canvas" ? current.name : null);

  const done = (next: Overview) => {
    qc.setQueryData(OVERVIEW_KEY, next);
    void qc.invalidateQueries({ queryKey: CANVAS_KEY });
  };
  const run = (f: () => Promise<Overview>) => {
    setBusy(true);
    f()
      .then(done, (e) => {
        toast(errorText(e), "error");
        void qc.invalidateQueries({ queryKey: OVERVIEW_KEY });
      })
      .finally(() => setBusy(false));
  };

  /** Shows something in the frame (an Explore Deck: a page of it, a board). */
  const go = (r: CanvasRow, where: { page?: string | null; board?: string | null } = {}) => {
    if (!o) return;
    const next: At = { kind: r.kind, name: r.name, page: where.page ?? null, board: where.board ?? null };
    if (!same(next, at)) setTreeShut(false);
    setAt(next);
    setLast(next);
    const url = addressOf(o.url, r, where);
    const win = frame.current?.contentWindow;
    if (src && win) win.location.replace(url);
    else setSrc(url);
  };

  // running: open the last thing looked at (else the first Explore Deck)
  useEffect(() => {
    if (!running) {
      setSrc(null);
      setAt(null);
      return;
    }
    if (at || !rows.length || !o) return;
    const r = rows.find((x) => same(x, last)) ?? rows.find((x) => x.kind === "canvas") ?? rows[0];
    const where = same(r, last) && last ? { page: last.page, board: last.board } : {};
    setAt({ kind: r.kind, name: r.name, page: where.page ?? null, board: where.board ?? null });
    setSrc(addressOf(o.url, r, where));
  }, [running, sections, o?.url, lastSaved]); // eslint-disable-line react-hooks/exhaustive-deps

  // the page being looked at is unfolded (a new canvas starts with only that one open)
  useEffect(() => {
    if (!at?.page) return;
    setOpenPages((cur) => (cur.includes(at.page!) ? cur : [...cur, at.page!]));
  }, [at?.page]);
  useEffect(() => setOpenPages(at?.page ? [at.page] : []), [at?.kind, at?.name]); // eslint-disable-line react-hooks/exhaustive-deps

  // the bench says where it went (another thing from its own menu, a page, a board opened or closed), or that
  // something was renamed or deleted (in its own UI, by an Agent): read the lists again
  useEffect(() => {
    if (!o) return;
    const onMessage = (e: MessageEvent) => {
      const d = e.data as { source?: string; type?: string; name?: string | null; page?: string | null; board?: string | null; ds?: string | null; motion?: string | null } | null;
      if (e.origin !== o.url || !d || d.source !== "dw") return;
      if (d.type === "changed") {
        void qc.invalidateQueries({ queryKey: CANVAS_KEY });
        return;
      }
      if (d.type !== "route") return;
      let next: At;
      if (d.ds) next = { kind: "ds", name: d.ds, page: null, board: null };
      else if (d.motion) next = { kind: "motion", name: d.motion, page: null, board: null };
      else if (d.name) {
        const kind = rowsRef.current.find((r) => r.name === d.name && (r.kind === "canvas" || r.kind === "page"))?.kind ?? "canvas";
        next = { kind, name: d.name, page: d.page ?? null, board: d.board ?? null };
      } else return;
      setAt((cur) => (cur && same(cur, next) && cur.page === next.page && cur.board === next.board ? cur : next));
      setLast(next);
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [o?.url]); // eslint-disable-line react-hooks/exhaustive-deps

  const pickFolder = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: "visual-atelier 文件夹" }).catch(() => null);
    if (typeof dir === "string") run(() => canvasApi.setFolder(dir));
  };
  const toggleSection = (id: string) => setClosed(closed.includes(id) ? closed.filter((x) => x !== id) : [...closed, id]);
  const togglePage = (id: string) => setOpenPages(openPages.includes(id) ? openPages.filter((x) => x !== id) : [...openPages, id]);

  const pages = detail?.pages ?? [];

  // ---------- 改名、删除 ----------
  const reread = async () => {
    await qc.invalidateQueries({ queryKey: CANVAS_KEY });
    return (qc.getQueryData<Section[]>([...CANVAS_KEY, "list"]) ?? []).flatMap((s) => s.items);
  };
  const acts = (x: number, y: number, r: CanvasRow, page: Page | null = null) =>
    setMenu({
      x,
      y,
      rows: [
        { label: "改名…", icon: <Pencil size={14} />, onSelect: () => setRenaming({ row: r, page }) },
        { label: page ? "删除这一页…" : "删除…", icon: <Trash2 size={14} />, onSelect: () => void remove(r, page) },
      ],
    });
  const below = (el: HTMLElement, r: CanvasRow, page: Page | null = null) => {
    const b = el.getBoundingClientRect();
    acts(b.left, b.bottom + 4, r, page);
  };
  const onRight = (r: CanvasRow, page: Page | null = null) => (e: React.MouseEvent) => {
    e.preventDefault();
    acts(e.clientX, e.clientY, r, page);
  };
  const rename = async (title: string) => {
    if (!renaming) return;
    await canvasApi.rename(renaming.row.name, title, renaming.page?.id ?? null);
    await reread();
    toast("改好了");
  };
  const remove = async (r: CanvasRow, page: Page | null) => {
    if (page && pages.length < 2) return toast("只剩这一页了：要删就删整块画布", "error");
    const n = page ? (detail?.boards.filter((b) => b.page === page.id).length ?? 0) : r.boards;
    const what = page ? `这一页的 ${n} 张板、便签和批准记录` : r.kind === "page" ? "这份问卷（连同填的答案）" : `${n} 张板和它们的批准记录`;
    const ok = await confirm({
      title: page ? `删除「${page.name}」这一页？` : `删除「${r.title}」？`,
      message: `${what}一起挪进 visual-atelier 的回收处，左栏上马上能撤销；以后要找回，让 Agent 从回收处放回。`,
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      const c = await canvasApi.remove(r.name, page?.id ?? null);
      const rest = await reread();
      if (same(r, at) && (!page || at?.page === page.id)) {
        const next = page ? r : (rest.find((x) => x.kind === "canvas") ?? rest.find((x) => x.kind === "page"));
        if (next) go(next);
      }
      if (c.id) {
        clearTimeout(undoTimer.current);
        setUndoable({ id: c.id, label: page ? `「${c.title}」这一页` : `「${c.title}」` });
        undoTimer.current = setTimeout(() => setUndoable(null), 12000);
      }
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const undo = async () => {
    if (!undoable) return;
    const id = undoable.id;
    setUndoable(null);
    try {
      const c = await canvasApi.restore(id);
      const r = (await reread()).find((x) => x.name === c.canvas && (x.kind === "canvas" || x.kind === "page"));
      if (r) go(r, { page: c.page });
      toast("放回去了");
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  useEffect(() => () => clearTimeout(undoTimer.current), []);
  /** Under the open Explore Deck: its pages (each folds), the boards of the open ones. */
  const tree = (r: CanvasRow) => {
    if (!detail || detail.name !== r.name) return null;
    const boardRow = (b: (typeof detail.boards)[number]) => {
      const picked = at?.board === b.file;
      return (
        <button key={b.file} type="button" aria-current={picked} title={b.file} onClick={() => go(r, { page: b.page, board: b.file })} className={`side-row cv-row cv-board w-full ${picked ? "on" : ""}`}>
          {picked && <Picked seed={1432} />}
          <span className="min-w-0 flex-1 truncate">{b.title}</span>
        </button>
      );
    };
    if (pages.length < 2) return detail.boards.map(boardRow);
    return pages.map((p) => {
      const open = openPages.includes(p.id);
      const here = at?.page === p.id;
      const boards = detail.boards.filter((b) => b.page === p.id);
      return (
        <div key={p.id}>
          <div className="cv-page-row" onContextMenu={onRight(r, p)}>
            <Fold open={open} label={p.name} onToggle={() => togglePage(p.id)} />
            <button type="button" aria-current={here && !at?.board} title={p.name} onClick={() => go(r, { page: p.id })} className={`cv-page ${here ? "on" : ""}`}>
              {p.name}
            </button>
            <span className="cv-count">{boards.length}</span>
            <MoreButton label={p.name} onOpen={(el) => below(el, r, p)} />
          </div>
          {open && boards.map(boardRow)}
        </div>
      );
    });
  };

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader
        icon={m.icon}
        color={m.color}
        title={m.label}
        right={
          running && (
            <span className="flex items-center justify-end gap-1">
              <RefreshButton label="重新读画布" onRefresh={() => qc.invalidateQueries({ queryKey: CANVAS_KEY })} />
              <Button variant="ghost" disabled={busy} onClick={() => run(canvasApi.stop)}>
                <Square size={13} /> 停止
              </Button>
            </span>
          )
        }
      />
      {o &&
        (running ? (
          <div className="flex min-h-0 flex-1 gap-5 pr-6 pb-5 pl-8">
            <nav aria-label="画布" className="side-list flex w-56 shrink-0 flex-col pr-4">
              {undoable && (
                <div role="status" className="cv-undo">
                  <span className="min-w-0 flex-1 truncate">删了{undoable.label}</span>
                  <button type="button" onClick={() => void undo()}>
                    <RotateCcw size={13} /> 撤销
                  </button>
                </div>
              )}
              <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
                {sections.map((sec) => {
                  const open = !closed.includes(sec.id);
                  return (
                    <section key={sec.id} aria-label={sec.title} className="cv-sec">
                      <button type="button" aria-expanded={open} onClick={() => toggleSection(sec.id)} className="cv-sec-head">
                        <ChevronRight size={13} className={`shrink-0 transition-transform duration-150 ${open ? "rotate-90" : ""}`} />
                        <span className="min-w-0 flex-1 truncate">{sec.title}</span>
                        <span className="cv-count">{sec.items.length}</span>
                      </button>
                      {open &&
                        (sec.items.length ? (
                          sec.items.map((r) => {
                            const on = same(r, at);
                            const deck = on && r.kind === "canvas";
                            const plain = on && !at?.board && (!at?.page || pages.length < 2);
                            const editable = r.kind === "canvas" || r.kind === "page";
                            return (
                              <div key={`${r.kind}:${r.name}`}>
                                <div className="cv-item" onContextMenu={editable ? onRight(r) : undefined}>
                                  {deck ? <Fold open={!treeShut} label={r.title} onToggle={() => setTreeShut(!treeShut)} /> : <span className="cv-fold" aria-hidden />}
                                  <button type="button" aria-current={plain} title={`${r.title}（${r.sub}）`} onClick={() => go(r)} className={`side-row cv-row min-w-0 flex-1 ${plain ? "on" : ""}`}>
                                    {plain && <Picked seed={1431} />}
                                    <span className="min-w-0 flex-1 truncate">{r.title}</span>
                                    {r.kind === "canvas" && <span className="cv-count">{r.boards}</span>}
                                  </button>
                                  {editable && <MoreButton label={r.title} onOpen={(el) => below(el, r)} />}
                                </div>
                                {deck && !treeShut && <div className="cv-tree">{tree(r)}</div>}
                              </div>
                            );
                          })
                        ) : (
                          <p className="cv-none">没有</p>
                        ))}
                    </section>
                  );
                })}
              </div>
            </nav>
            <section className="cv-frame min-w-0 flex-1 rounded-card border bg-card">{src && <iframe ref={frame} src={src} title="画布" />}</section>
          </div>
        ) : (
          <Stopped o={o} busy={busy} onStart={() => run(canvasApi.start)} onFolder={() => void pickFolder()} onDefault={() => run(() => canvasApi.setFolder(""))} />
        ))}
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
      {renaming && (
        <RenameDialog
          title={renaming.page ? `给「${renaming.page.name}」改名` : `给「${renaming.row.title}」改名`}
          initial={renaming.page?.name ?? renaming.row.title}
          hint={renaming.page ? undefined : "只改名字，地址不变。"}
          onSubmit={rename}
          onClose={() => setRenaming(null)}
        />
      )}
      {confirmDialog}
    </div>
  );
}
