// 看画布: visual-atelier's canvases inside Pomelo (Dawn Plan · Phase 8). The canvases are shown by that repository's
// own viewer, the bench — a local server whose page is the canvas UI. Here: 启动 / 停止 it (Rust, canvas.rs), a
// side column of the canvases (the open one lists its boards, by page), and the bench's UI in a frame beside it —
// pan and zoom, pages, one board full size, Tweaks, 批准 all happen in there, nothing opens a browser.
// The column and the frame follow each other: a click here moves the frame (its address is replaced, no history
// step); the bench tells this page where it went (window message `{ source: "dw", type: "route" }`).
// Nothing is kept except the last canvas looked at (setting `canvas.last`) and, when changed, the folder.
import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Play, Square } from "@/shared/icons";
import { moduleById } from "@/app/modules";
import { errorText, useToast } from "@/shared/feedback";
import { useSetting } from "@/shared/settings";
import { Button, EmptyState, PageHeader, Picked, RefreshButton } from "@/shared/ui";
import { addressOf, CANVAS_KEY, canvasApi, OVERVIEW_KEY, useCanvasBoards, useCanvasList, useCanvasOverview, type CanvasRow, type Overview } from "./api";
import "./canvas.css";

type At = { name: string; page: string | null; board: string | null };
const isAt = (v: unknown): v is At | null => v === null || (!!v && typeof (v as At).name === "string");

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

export default function CanvasPage() {
  const m = moduleById("canvas");
  const qc = useQueryClient();
  const toast = useToast();
  const { data: o } = useCanvasOverview();
  const running = o?.state === "running";
  const { data: rows = [] } = useCanvasList(running);
  const [last, setLast] = useSetting<At | null>("canvas.last", null, isAt);
  const [at, setAt] = useState<At | null>(null);
  const [busy, setBusy] = useState(false);
  const frame = useRef<HTMLIFrameElement>(null);
  /** The address the frame was given first; later moves replace its location instead (no reload, no history step). */
  const [src, setSrc] = useState<string | null>(null);

  const row = (name: string | undefined) => rows.find((r) => r.name === name);
  const current = row(at?.name);
  const { data: detail } = useCanvasBoards(running && current?.kind === "canvas", current?.name ?? null);

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

  /** Shows a canvas (a page of it, a board) in the frame. */
  const go = (r: CanvasRow, where: { page?: string | null; board?: string | null } = {}) => {
    if (!o) return;
    const next = { name: r.name, page: where.page ?? null, board: where.board ?? null };
    setAt(next);
    setLast(next);
    const url = addressOf(o.url, r, where);
    const win = frame.current?.contentWindow;
    if (src && win) win.location.replace(url);
    else setSrc(url);
  };

  // running: open the last canvas looked at (else the first canvas with boards, before any 问卷 page)
  useEffect(() => {
    if (!running) {
      setSrc(null);
      setAt(null);
      return;
    }
    if (at || !rows.length || !o) return;
    const r = row(last?.name) ?? rows.find((x) => x.kind === "canvas") ?? rows[0];
    const where = r.name === last?.name ? { page: last.page, board: last.board } : {};
    setAt({ name: r.name, page: where.page ?? null, board: where.board ?? null });
    setSrc(addressOf(o.url, r, where));
  }, [running, rows, o?.url]); // eslint-disable-line react-hooks/exhaustive-deps

  // the bench says where it went (another canvas from its own menu, a page, a board opened or closed)
  useEffect(() => {
    if (!o) return;
    const onMessage = (e: MessageEvent) => {
      const d = e.data as { source?: string; type?: string; name?: string | null; page?: string | null; board?: string | null } | null;
      if (e.origin !== o.url || !d || d.source !== "dw" || d.type !== "route" || !d.name) return;
      const next = { name: d.name, page: d.page ?? null, board: d.board ?? null };
      setAt((cur) => (cur && cur.name === next.name && cur.page === next.page && cur.board === next.board ? cur : next));
      setLast(next);
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [o?.url]); // eslint-disable-line react-hooks/exhaustive-deps

  const pickFolder = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: "visual-atelier 文件夹" }).catch(() => null);
    if (typeof dir === "string") run(() => canvasApi.setFolder(dir));
  };

  const pages = detail?.pages ?? [];
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
            <nav aria-label="画布" className="side-list flex w-52 shrink-0 flex-col pr-4">
              <div className="-m-1 min-h-0 flex-1 space-y-0.5 overflow-y-auto p-1">
                {rows.map((r) => {
                  const on = r.name === at?.name;
                  return (
                    <div key={r.name}>
                      <button type="button" aria-current={on && !at?.board} title={r.name} onClick={() => go(r)} className={`side-row cv-row w-full ${on && !at?.board ? "on" : ""}`}>
                        {on && !at?.board && <Picked seed={1431} />}
                        <span className="min-w-0 flex-1 truncate">{r.title}</span>
                        {r.kind === "canvas" && <span className="shrink-0 text-xs font-normal text-muted-foreground">{r.boards}</span>}
                      </button>
                      {on &&
                        r.kind === "canvas" &&
                        detail?.name === r.name &&
                        (pages.length ? pages : [null]).map((p) => (
                          <div key={p?.id ?? "all"}>
                            {pages.length > 1 && p && (
                              <button type="button" onClick={() => go(r, { page: p.id })} className={`cv-page ${at?.page === p.id && !at.board ? "on" : ""}`}>
                                {p.name}
                              </button>
                            )}
                            {detail.boards
                              .filter((b) => !p || b.page === p.id)
                              .map((b) => {
                                const picked = at?.board === b.file;
                                return (
                                  <button key={b.file} type="button" aria-current={picked} title={b.file} onClick={() => go(r, { page: b.page, board: b.file })} className={`side-row cv-row cv-board w-full ${picked ? "on" : ""}`}>
                                    {picked && <Picked seed={1432} />}
                                    <span className="min-w-0 flex-1 truncate">{b.title}</span>
                                  </button>
                                );
                              })}
                          </div>
                        ))}
                    </div>
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
