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
// 改名、删除（站主 2026-11-13）：Explore Deck、问卷页和一块画布的每一页，右键或行尾的「⋯」；11-14 起和 bench 一样，画板也能
// 改名、删除，Design System、Motion Deck 能改名（只改 bench 和这里显示的名字，方案的源和云端不变；「回到原名」）。做的是
// visual-atelier 的 bench（canvas.rs → 它的接口），删了的进它的回收处；左栏顶上一条「撤销」放回去。bench 自己的界面里也
// 能改，改了它发 `{ source: "dw", type: "changed" }`，这边重新读清单。
// visual-atelier 的 Phase 2（api 5，站主 11-15）：第五类 Checks（`npm run all` 的结果，只有一行，不能改名删除）；开着的页下面每张画板后面
// 标它的批准状态（待批、已批准、退回，改过的加「· 改过」，退回的悬停看原因）。批准、退回还是在右边的框里点，这边只显示，跟着 bench 发的
// `{ source: "dw", type: "reviews" }` 变。
// Nothing is kept except the last thing looked at (setting `canvas.last`), the folded lists and, when changed, the folder.
import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ChevronRight, Ellipsis, FolderOpen, Pencil, Play, RotateCcw, Square, Trash2 } from "@/shared/icons";
import { moduleById } from "@/app/modules";
import { Menu, type MenuAt } from "@/app/Menu";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { isStringArray, useSetting } from "@/shared/settings";
import { InlineName } from "@/shared/InlineName";
import { Button, EmptyState, PageHeader, Picked, RefreshButton } from "@/shared/ui";
import { Fold, foldByHand } from "@/shared/Fold";
import { addressOf, CANVAS_KEY, canvasApi, OVERVIEW_KEY, reviewsOf, useCanvasBoards, useCanvasList, useCanvasOverview, type Board, type CanvasRow, type Detail, type Kind, type Overview, type Page, type Review, type Section } from "./api";
import "./canvas.css";

type At = { kind: Kind; name: string; page: string | null; board: string | null };
const KINDS: Kind[] = ["ds", "motion", "canvas", "page", "checks"];
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

/** 行尾的「⋯」：指着这一行才露出来。`what`：它里面有什么（Design System、Motion Deck 只有改名）。 */
function MoreButton(props: { label: string; what?: string; onOpen: (el: HTMLElement) => void }) {
  const what = props.what ?? "改名、删除";
  return (
    <button
      type="button"
      aria-label={`${props.label}：${what}`}
      title={what}
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

const REVIEW: Record<Review["status"], string> = { proposed: "待批", approved: "已批准", rejected: "退回" };

/** A board's 批准状态 after its title, as the bench words it; 退回 shows why on hover. */
function ReviewMark({ r }: { r: Review }) {
  const label = `${REVIEW[r.status]}${r.changed ? " · 改过" : ""}`;
  const tip = [r.note && `${r.status === "rejected" ? "原因" : "备注"}：${r.note}`, r.changed && "标完以后这张板又改过"].filter(Boolean).join("\n");
  return (
    <span className={`cv-review ${r.status}`} title={tip || undefined}>
      {label}
    </span>
  );
}

/** The little arrow in front of something that folds. */
function FoldArrow(props: { open: boolean; label: string; onToggle: () => void }) {
  return (
    <button
      type="button"
      aria-expanded={props.open}
      aria-label={`${props.open ? "收起" : "展开"}${props.label}`}
      onClick={() => {
        foldByHand();
        props.onToggle();
      }}
      className="cv-fold"
    >
      <ChevronRight size={13} className={`pm-fold-chev ${props.open ? "rotate-90" : ""}`} />
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
  const [renaming, setRenaming] = useState<{ row: CanvasRow; page: Page | null; board: Board | null } | null>(null);
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
      const d = e.data as { source?: string; type?: string; name?: string | null; page?: string | null; board?: string | null; ds?: string | null; motion?: string | null; checks?: boolean; reviews?: unknown } | null;
      if (e.origin !== o.url || !d || d.source !== "dw") return;
      if (d.type === "changed") {
        void qc.invalidateQueries({ queryKey: CANVAS_KEY });
        return;
      }
      if (d.type === "reviews" && d.name) {
        const rs = reviewsOf(d.reviews);
        qc.setQueryData<Detail>([...CANVAS_KEY, "boards", d.name], (old) => old && { ...old, boards: old.boards.map((b) => ({ ...b, review: rs[b.file] ?? null })) });
        return;
      }
      if (d.type !== "route") return;
      let next: At;
      if (d.checks) next = { kind: "checks", name: rowsRef.current.find((r) => r.kind === "checks")?.name ?? "all", page: null, board: null };
      else if (d.ds) next = { kind: "ds", name: d.ds, page: null, board: null };
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
  /** A Design System or Motion Deck: only its name (in the bench), and back to its own. */
  const scheme = (r: CanvasRow) => r.kind === "ds" || r.kind === "motion";
  const acts = (x: number, y: number, r: CanvasRow, page: Page | null = null, board: Board | null = null) =>
    setMenu({
      x,
      y,
      rows: scheme(r)
        ? [
            { label: "改名", icon: <Pencil size={14} />, onSelect: () => setRenaming({ row: r, page: null, board: null }) },
            ...(r.schemeTitle ? [{ label: "回到原名", icon: <RotateCcw size={14} />, onSelect: () => void ownName(r) }] : []),
          ]
        : [
            { label: "改名", icon: <Pencil size={14} />, onSelect: () => setRenaming({ row: r, page, board }) },
            { label: board ? "删除这张…" : page ? "删除这一页…" : "删除…", icon: <Trash2 size={14} />, onSelect: () => void (board ? removeBoard(r, board) : remove(r, page)) },
          ],
    });
  const below = (el: HTMLElement, r: CanvasRow, page: Page | null = null, board: Board | null = null) => {
    const b = el.getBoundingClientRect();
    acts(b.left, b.bottom + 4, r, page, board);
  };
  const onRight = (r: CanvasRow, page: Page | null = null, board: Board | null = null) => (e: React.MouseEvent) => {
    e.preventDefault();
    acts(e.clientX, e.clientY, r, page, board);
  };
  /** 改名就在那一行里改（站主 11-15：不要弹窗和说明）；Design System、Motion Deck 清空就是回到原名。 */
  const isRenaming = (r: CanvasRow, page: Page | null, board: Board | null) =>
    !!renaming && same(renaming.row, r) && (renaming.page?.id ?? null) === (page?.id ?? null) && (renaming.board?.file ?? null) === (board?.file ?? null);
  const renameBox = (r: CanvasRow, page: Page | null, board: Board | null, initial: string) => (
    <InlineName
      initial={initial}
      label={`「${initial}」改名`}
      clearable={scheme(r)}
      onDone={(title) => {
        setRenaming(null);
        if (title === null) return;
        const job = scheme(r) ? canvasApi.nameScheme(r.kind as "ds" | "motion", r.name, title || null) : canvasApi.rename(r.name, title, page?.id ?? null, board?.file ?? null);
        job.then(reread, (e) => toast(errorText(e), "error"));
      }}
    />
  );
  const ownName = async (r: CanvasRow) => {
    try {
      await canvasApi.nameScheme(r.kind as "ds" | "motion", r.name, null);
      await reread();
      toast("回到原名了");
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  /** Shows the undo bar for what was just deleted. */
  const offerUndo = (id: string | null, label: string) => {
    if (!id) return;
    clearTimeout(undoTimer.current);
    setUndoable({ id, label });
    undoTimer.current = setTimeout(() => setUndoable(null), 12000);
  };
  const removeBoard = async (r: CanvasRow, b: Board) => {
    const ok = await confirm({ title: `删除画板「${b.title}」？`, confirmLabel: "删除", danger: true });
    if (!ok) return;
    try {
      const c = await canvasApi.remove(r.name, null, b.file);
      await reread();
      if (same(r, at) && at?.board === b.file) go(r, { page: b.page });
      offerUndo(c.id, `画板「${c.title || b.title}」`);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const remove = async (r: CanvasRow, page: Page | null) => {
    if (page && pages.length < 2) return toast("只剩这一页了：要删就删整块画布", "error");
    const n = page ? (detail?.boards.filter((b) => b.page === page.id).length ?? 0) : r.boards;
    const ok = await confirm({
      title: page ? `删除「${page.name}」这一页？` : `删除「${r.title}」？`,
      message: r.kind === "page" ? "连同填的答案。" : n ? `连同 ${n} 张板。` : undefined,
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
      offerUndo(c.id, page ? `「${c.title}」这一页` : `「${c.title}」`);
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
      if (r) go(r, { page: c.page, board: c.file });
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
        <div key={b.file} className="cv-item" onContextMenu={onRight(r, null, b)}>
          {isRenaming(r, null, b) ? (
            <div className="cv-board min-w-0 flex-1 py-0.5">{renameBox(r, null, b, b.title)}</div>
          ) : (
            <button type="button" aria-current={picked} title={b.file} onClick={() => go(r, { page: b.page, board: b.file })} className={`side-row cv-row cv-board min-w-0 flex-1 ${picked ? "on" : ""}`}>
              {picked && <Picked seed={1432} />}
              <span className="min-w-0 flex-1 truncate">{b.title}</span>
              {b.review && <ReviewMark r={b.review} />}
            </button>
          )}
          <MoreButton label={b.title} onOpen={(el) => below(el, r, null, b)} />
        </div>
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
            <FoldArrow open={open} label={p.name} onToggle={() => togglePage(p.id)} />
            {isRenaming(r, p, null) ? (
              <div className="min-w-0 flex-1">{renameBox(r, p, null, p.name)}</div>
            ) : (
              <button type="button" aria-current={here && !at?.board} title={p.name} onClick={() => go(r, { page: p.id })} className={`cv-page ${here ? "on" : ""}`}>
                {p.name}
              </button>
            )}
            <span className="cv-count">{boards.length}</span>
            <MoreButton label={p.name} onOpen={(el) => below(el, r, p)} />
          </div>
          <Fold open={open}>{boards.map(boardRow)}</Fold>
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
                      <button
                        type="button"
                        aria-expanded={open}
                        onClick={() => {
                          foldByHand();
                          toggleSection(sec.id);
                        }}
                        className="cv-sec-head"
                      >
                        <ChevronRight size={13} className={`pm-fold-chev shrink-0 ${open ? "rotate-90" : ""}`} />
                        <span className="min-w-0 flex-1 truncate">{sec.title}</span>
                        <span className="cv-count">{sec.items.length}</span>
                      </button>
                      <Fold open={open}>
                        {sec.items.length ? (
                          sec.items.map((r) => {
                            const on = same(r, at);
                            const deck = on && r.kind === "canvas";
                            const plain = on && !at?.board && (!at?.page || pages.length < 2);
                            const fixed = r.kind === "checks";
                            return (
                              <div key={`${r.kind}:${r.name}`}>
                                <div className="cv-item" onContextMenu={fixed ? undefined : onRight(r)}>
                                  {deck ? <FoldArrow open={!treeShut} label={r.title} onToggle={() => setTreeShut(!treeShut)} /> : <span className="cv-fold" aria-hidden />}
                                  {isRenaming(r, null, null) ? (
                                    <div className="min-w-0 flex-1 py-0.5">{renameBox(r, null, null, r.title)}</div>
                                  ) : (
                                    <button type="button" aria-current={plain} title={`${r.title}（${r.sub}）`} onClick={() => go(r)} className={`side-row cv-row min-w-0 flex-1 ${plain ? "on" : ""}`}>
                                      {plain && <Picked seed={1431} />}
                                      <span className="min-w-0 flex-1 truncate">{r.title}</span>
                                      {r.kind === "canvas" && <span className="cv-count">{r.boards}</span>}
                                    </button>
                                  )}
                                  {!fixed && <MoreButton label={r.title} what={scheme(r) ? "改名" : undefined} onOpen={(el) => below(el, r)} />}
                                </div>
                                {deck && (
                                  <Fold open={!treeShut} className="cv-tree">
                                    {tree(r)}
                                  </Fold>
                                )}
                              </div>
                            );
                          })
                        ) : (
                          <p className="cv-none">没有</p>
                        )}
                      </Fold>
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
      {confirmDialog}
    </div>
  );
}
