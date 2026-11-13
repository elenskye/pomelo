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
// Nothing is kept except the last thing looked at (setting `canvas.last`), the folded lists and, when changed, the folder.
import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ChevronRight, FolderOpen, Play, Square } from "@/shared/icons";
import { moduleById } from "@/app/modules";
import { errorText, useToast } from "@/shared/feedback";
import { isStringArray, useSetting } from "@/shared/settings";
import { Button, EmptyState, PageHeader, Picked, RefreshButton } from "@/shared/ui";
import { addressOf, CANVAS_KEY, canvasApi, OVERVIEW_KEY, useCanvasBoards, useCanvasList, useCanvasOverview, type CanvasRow, type Kind, type Overview } from "./api";
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

  // the bench says where it went (another thing from its own menu, a page, a board opened or closed)
  useEffect(() => {
    if (!o) return;
    const onMessage = (e: MessageEvent) => {
      const d = e.data as { source?: string; type?: string; name?: string | null; page?: string | null; board?: string | null; ds?: string | null; motion?: string | null } | null;
      if (e.origin !== o.url || !d || d.source !== "dw" || d.type !== "route") return;
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
          <div className="cv-page-row">
            <Fold open={open} label={p.name} onToggle={() => togglePage(p.id)} />
            <button type="button" aria-current={here && !at?.board} title={p.name} onClick={() => go(r, { page: p.id })} className={`cv-page ${here ? "on" : ""}`}>
              {p.name}
            </button>
            <span className="cv-count">{boards.length}</span>
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
                            return (
                              <div key={`${r.kind}:${r.name}`}>
                                <div className="cv-item">
                                  {deck ? <Fold open={!treeShut} label={r.title} onToggle={() => setTreeShut(!treeShut)} /> : <span className="cv-fold" aria-hidden />}
                                  <button type="button" aria-current={plain} title={`${r.title}（${r.sub}）`} onClick={() => go(r)} className={`side-row cv-row min-w-0 flex-1 ${plain ? "on" : ""}`}>
                                    {plain && <Picked seed={1431} />}
                                    <span className="min-w-0 flex-1 truncate">{r.title}</span>
                                    {r.kind === "canvas" && <span className="cv-count">{r.boards}</span>}
                                  </button>
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
    </div>
  );
}
