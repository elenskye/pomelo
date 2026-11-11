// Atlas: 柚子's 年度总结 as a 年度小报 (requirements「以后 · Atlas」; the designs come frozen from visual-atelier, designs.ts).
// Three views on one page (the header's 「全部年份」 goes back):
// - the years: one cover a year, in a row that scrolls sideways; a cover is its newest run (which design it was made
//   for is printed on it) and a click opens that one. Under it the 时间穿梭 button goes to the year's own view;
// - a year: every run ever written for it (a year done again — 补做 — is one more run, older ones stay), newest first,
//   each a cover; a click opens it, × deletes it (Rust atlas_delete, undoable: no AI tool, no schedule can);
// - 样式: every design the app carries, newest first; a click draws the newest run made for it (or, with none, the
//   newest run there is) with that design, to look at it. A run itself is always drawn with the design it was made for.
// 立即运行 writes this year up to today; 补做 writes a past year. The 小报 opens over the desk (Journey.tsx).
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { moduleById } from "@/app/modules";
import { useTabs } from "@/app/tabs";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { reducedMotion } from "@/shared/hand";
import { ChevronLeft, ChevronRight, Play, Plus, TimeTravel, X } from "@/shared/icons";
import { api as ipc } from "@/shared/ipc";
import { useOpenRequest, type Hit } from "@/shared/open";
import { Button, EmptyState, IconButton, PageHeader } from "@/shared/ui";
import { ATLAS_KEY, atlasApi, OVERVIEW_KEY, useAtlasOverview, type Entry } from "./api";
import { designLabel, designList } from "./designs";
import { Journey } from "./Journey";
import "./atlas.css";

/** A record of the 小报 (`note:12`) → the module page that shows it and what it asks for. */
const RECORDS: Record<string, [module: string, kind: string]> = { note: ["notes", "note"], goal: ["todo", "goal"], trip: ["packing", "trip"], offer: ["jobs", "job"], day: ["days", "day"], book: ["books", "book"], film: ["films", "film"], course: ["drawing", "course"], talk: ["youzi", "session"] };

const md = (d: string) => `${Number(d.slice(5, 7))} 月 ${Number(d.slice(8, 10))} 日`;
/** The local day a run was written. */
function wrote(e: Entry) {
  const t = new Date(e.createdAt);
  return `${t.getFullYear()} 年 ${t.getMonth() + 1} 月 ${t.getDate()} 日`;
}
const span = (e: Entry) => (e.until ? `截至 ${md(e.until)}` : "全年");
const told = (e: Entry) => (e.scenes ? `${e.scenes} 幕` : "各幕没写成");
const TILTS = [-2, 1.5, -1];

type View = { at: "years" } | { at: "year"; year: string } | { at: "designs" };

/** A printed cover: `big` (the year, or a design's version) under `small`, `foot` in the corner, `stamp` when the year wasn't over. */
function Cover(props: { big: string; small: string; foot?: string; stamp?: string; stack?: boolean; tilt: number }) {
  return (
    <span className="atlas-cover" style={{ "--tilt": `${props.tilt}deg` } as React.CSSProperties}>
      {props.stack && <span className="atlas-cover-under" aria-hidden />}
      <span className="atlas-cover-paper" aria-hidden />
      <span className="atlas-cover-name">{props.small}</span>
      <span className="atlas-cover-year">{props.big}</span>
      <span className="atlas-cover-bar" aria-hidden />
      {props.stamp && <span className="atlas-cover-stamp">{props.stamp}</span>}
      {props.foot && <span className="atlas-cover-foot">{props.foot}</span>}
    </span>
  );
}

/** A row that scrolls sideways: two fingers, a mouse wheel (up / down moves it too), or the arrows at its ends. */
function Strip(props: { label: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [more, setMore] = useState({ left: false, right: false });
  const measure = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    const next = { left: el.scrollLeft > 4, right: el.scrollLeft + el.clientWidth < el.scrollWidth - 4 };
    setMore((old) => (old.left === next.left && old.right === next.right ? old : next));
  }, []);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (Math.abs(e.deltaY) <= Math.abs(e.deltaX) || el.scrollWidth <= el.clientWidth) return;
      e.preventDefault();
      el.scrollLeft += e.deltaY;
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => {
      el.removeEventListener("wheel", onWheel);
      watch.disconnect();
    };
  }, [measure]);
  // what's in the row changed
  useEffect(measure);
  const go = (dir: number) => {
    const el = ref.current;
    el?.scrollBy({ left: dir * Math.max(200, el.clientWidth * 0.7), behavior: reducedMotion() ? "auto" : "smooth" });
  };
  return (
    <div className="atlas-strip-wrap">
      <div ref={ref} className="atlas-strip" role="list" aria-label={props.label} onScroll={measure}>
        {props.children}
      </div>
      {more.left && (
        <span className="atlas-strip-go is-left">
          <IconButton label="往左" onClick={() => go(-1)}>
            <ChevronLeft size={16} />
          </IconButton>
        </span>
      )}
      {more.right && (
        <span className="atlas-strip-go is-right">
          <IconButton label="往右" onClick={() => go(1)}>
            <ChevronRight size={16} />
          </IconButton>
        </span>
      )}
    </div>
  );
}

export default function AtlasPage() {
  const m = moduleById("atlas");
  const qc = useQueryClient();
  const toast = useToast();
  const tabs = useTabs();
  const [confirm, confirmDialog] = useConfirm();
  const { data: o } = useAtlasOverview();
  const [open, setOpen] = useState<{ id: number; desk: boolean; design?: string } | null>(null);
  const [view, setView] = useState<View>({ at: "years" });
  // 首页's envelope (kind "envelope": the envelope first) and anything else that names a run
  useOpenRequest((h) => setOpen({ id: h.id, desk: h.kind === "envelope" }));
  // testing in the real window without clicking (docs/testing.md「真实窗口」): the setting `dev.atlas` names a run to
  // open once, like `dev.open` names the page
  useEffect(() => {
    void ipc
      .settingsGet<unknown>("dev.atlas")
      .then((v) => {
        if (typeof v !== "number") return;
        void ipc.settingsSet("dev.atlas", null).catch(() => {});
        setOpen({ id: v, desk: false });
      })
      .catch(() => {});
    // `dev.atlasRun` (docs/testing.md「Atlas」真模型): `true` writes this year up to today, "2025" 补做 that year, once
    void ipc
      .settingsGet<unknown>("dev.atlasRun")
      .then((v) => {
        if (v !== true && typeof v !== "string") return;
        void ipc.settingsSet("dev.atlasRun", null).catch(() => {});
        void atlasApi.run(v === true ? null : v).catch(() => {});
      })
      .catch(() => {});
    // `dev.atlasView`: "designs" or a year, the view to start on
    void ipc
      .settingsGet<unknown>("dev.atlasView")
      .then((v) => {
        if (typeof v !== "string") return;
        void ipc.settingsSet("dev.atlasView", null).catch(() => {});
        setView(v === "designs" ? { at: "designs" } : { at: "year", year: v });
      })
      .catch(() => {});
  }, []);

  if (!o) return <div className="flex h-full flex-col"><PageHeader icon={m.icon} color={m.color} title={m.label} /></div>;
  const thisYear = o.today.slice(0, 4);
  const newest = (a: Entry, b: Entry) => b.createdAt.localeCompare(a.createdAt) || b.id - a.id;
  const byYear = new Map<string, Entry[]>();
  for (const e of [...o.entries].sort(newest)) byYear.set(e.year, [...(byYear.get(e.year) ?? []), e]);
  // every year with a run or with records, newest first; this year always
  const years = [...new Set([thisYear, ...byYear.keys(), ...o.years.map((y) => y.year)])].filter((y) => y <= thisYear).sort((a, b) => b.localeCompare(a));
  const records = new Map(o.years.map((y) => [y.year, y.records]));
  // a year whose last run was deleted has no view of its own any more
  const at: View = view.at === "year" && !byYear.has(view.year) && o.writing !== view.year ? { at: "years" } : view;
  const runs = at.at === "year" ? (byYear.get(at.year) ?? []) : [];
  const refresh = () => void qc.invalidateQueries({ queryKey: ATLAS_KEY });

  const run = async (y: string) => {
    const now = y === thisYear;
    const again = byYear.has(y);
    const ok = await confirm({
      title: now ? `写 ${y} 年到今天的年度小报？` : again ? `再做一份 ${y} 年的年度小报？` : `补做 ${y} 年的年度小报？`,
      message: (
        <>
          柚子照「年度总结」的流程把这一年读一遍、写一封信，再做成小报，用的是「定时」里年度总结选的模型，要等一会儿。
          {again && " 以前做过的那几份都留着。"}
          {now && " 今年还没过完，封面上会盖「截至今天」。"}
        </>
      ),
      confirmLabel: "开始写",
    });
    if (!ok) return;
    try {
      await atlasApi.run(now ? null : y);
      toast("柚子开始写了，写好了会出现在这里");
      void qc.invalidateQueries({ queryKey: OVERVIEW_KEY });
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const remove = async (e: Entry) => {
    const ok = await confirm({ title: `删除 ${wrote(e)} 写的这一份？`, message: `${e.year} 年 · ${span(e)} · ${told(e)}。删了能用 ⌘Z 撤销；「定时」里它的对话不受影响。`, confirmLabel: "删除", danger: true });
    if (!ok) return;
    try {
      await atlasApi.remove(e.id);
      refresh();
    } catch (err) {
      toast(errorText(err), "error");
    }
  };
  const openRecord = (key: string) => {
    const [kind, id] = key.split(":");
    const to = RECORDS[kind];
    if (!to || !Number(id)) return;
    const hit: Hit = { module: to[0], kind: to[1], id: Number(id), title: "", detail: null, parent: null, day: null, amount: null, currency: null, flow: null };
    tabs.open(`/${to[0]}`, { open: hit });
  };

  const total = o.entries.length;
  const back = (
    <Button variant="ghost" onClick={() => setView({ at: "years" })}>
      <ChevronLeft size={14} /> 全部年份
    </Button>
  );
  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        icon={m.icon}
        color={m.color}
        title={at.at === "year" ? `${m.label} · ${at.year} 年` : at.at === "designs" ? `${m.label} · 样式` : m.label}
        right={
          <span className="flex items-center justify-end gap-3">
            {at.at === "years" ? (
              <>
                {total > 0 && (
                  <span className="pointer-events-none">
                    <b className="text-base text-foreground">{byYear.size}</b> 年 · <b className="text-base text-foreground">{total}</b> 份
                  </span>
                )}
                <Button variant="ghost" onClick={() => setView({ at: "designs" })}>
                  样式
                </Button>
                <Button variant="primary" disabled={!!o.writing} onClick={() => void run(thisYear)}>
                  <Play size={14} /> 立即运行（写到今天）
                </Button>
              </>
            ) : at.at === "year" ? (
              <>
                {back}
                <Button disabled={!!o.writing} onClick={() => void run(at.year)}>
                  <Plus size={14} /> {at.year === thisYear ? "再写一份（到今天）" : "再做一份"}
                </Button>
              </>
            ) : (
              back
            )}
          </span>
        }
      />
      <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
        {at.at === "years" && (
          <>
            {total === 0 && !o.writing && <EmptyState title="还没有年度小报" />}
            <Strip label="每一年的年度小报">
              {years.map((y, i) => {
                const list = byYear.get(y) ?? [];
                const latest = list[0];
                const writing = o.writing === y;
                return (
                  <div key={y} role="listitem" className="atlas-item">
                    {latest ? (
                      <button type="button" className="pm-press" aria-label={`翻开 ${y} 年最新的一份`} onClick={() => setOpen({ id: latest.id, desk: false })}>
                        <Cover big={y} small="柚子的年度小报" foot={designLabel(latest.design)} stamp={latest.until ? `截至 ${md(latest.until)}` : undefined} stack={list.length > 1} tilt={TILTS[i % 3]} />
                      </button>
                    ) : (
                      <span className="atlas-cover is-none">{writing ? "正在写…" : "还没写"}</span>
                    )}
                    <div className="mt-3 font-bold tabular-nums">{y} 年</div>
                    <div className="text-xs whitespace-nowrap text-muted-foreground">{latest ? `${span(latest)} · ${told(latest)}` : records.get(y) ? `有 ${records.get(y)} 条记录` : "还没有记录"}</div>
                    <div className="mt-2 flex h-9 items-center gap-1">
                      {latest ? (
                        <>
                          <IconButton label={`时间穿梭：${y} 年做过的 ${list.length} 份`} onClick={() => setView({ at: "year", year: y })}>
                            <TimeTravel size={20} />
                          </IconButton>
                          <span className="text-xs text-muted-foreground tabular-nums">{writing ? "柚子在写…" : `${list.length} 份`}</span>
                        </>
                      ) : writing ? (
                        <span className="text-sm text-muted-foreground">柚子在写…</span>
                      ) : (
                        <Button disabled={!!o.writing} onClick={() => void run(y)}>
                          {y === thisYear ? "写到今天" : "补做这一年"}
                        </Button>
                      )}
                    </div>
                  </div>
                );
              })}
            </Strip>
          </>
        )}
        {at.at === "year" && (
          <Strip label={`${at.year} 年做过的每一份`}>
            {o.writing === at.year && (
              <div role="listitem" className="atlas-item is-run">
                <span className="atlas-cover is-none">正在写…</span>
                <div className="mt-3 text-sm text-muted-foreground">柚子在写…</div>
              </div>
            )}
            {runs.map((e, i) => (
              <div key={e.id} role="listitem" className="atlas-item is-run">
                <button type="button" className="pm-press" aria-label={`翻开 ${wrote(e)} 写的这一份`} onClick={() => setOpen({ id: e.id, desk: false })}>
                  <Cover big={e.year} small="柚子的年度小报" foot={designLabel(e.design)} stamp={e.until ? `截至 ${md(e.until)}` : undefined} tilt={TILTS[i % 3]} />
                </button>
                <div className="mt-3 flex items-center justify-between gap-1">
                  <span className="font-bold whitespace-nowrap tabular-nums">{wrote(e)}</span>
                  <IconButton label={`删除 ${wrote(e)} 写的这一份`} danger onClick={() => void remove(e)}>
                    <X size={14} />
                  </IconButton>
                </div>
                <div className="text-xs text-muted-foreground">
                  {span(e)} · {told(e)}
                </div>
                <div className="truncate text-xs text-muted-foreground">{e.modelName}</div>
                {e.error && <div className="selectable mt-1 text-xs leading-5 text-destructive">{e.error}</div>}
              </div>
            ))}
          </Strip>
        )}
        {at.at === "designs" && (
          <Strip label="Atlas 的每一版样式">
            {designList().map((d, i) => {
              const made = o.entries.filter((e) => e.design === d.key).sort(newest);
              // looked at with the newest run made for it, else with the newest run there is
              const sample = made[0] ?? [...o.entries].sort(newest)[0];
              return (
                <div key={d.key} role="listitem" className="atlas-item is-design">
                  <button type="button" className="pm-press" disabled={!sample} aria-label={`用 ${designLabel(d.key)} 翻开一份看看`} onClick={() => sample && setOpen({ id: sample.id, desk: false, design: d.key })}>
                    <Cover big={d.version} small={d.name} foot={d.frozen ? `${d.frozen} 定稿` : undefined} tilt={TILTS[i % 3]} />
                  </button>
                  <div className="mt-3 font-bold tabular-nums">
                    {designLabel(d.key)}
                    {d.key === o.design && <span className="ml-2 text-xs font-normal text-muted-foreground">新做的用这一版</span>}
                  </div>
                  <div className="text-xs text-muted-foreground">{made.length ? `${made.length} 份是照它做的` : "还没有照它做的"}</div>
                  {d.note && <p className="mt-1 text-xs leading-5 text-muted-foreground">{d.note}</p>}
                </div>
              );
            })}
          </Strip>
        )}
      </div>
      {open && <Journey key={`${open.id}:${open.design ?? ""}`} id={open.id} desk={open.desk} design={open.design} onOpenRecord={openRecord} onClose={() => setOpen(null)} />}
      {confirmDialog}
    </div>
  );
}
