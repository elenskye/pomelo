// Atlas: 柚子's 年度总结 as a 年度小报 (requirements「以后 · Atlas」; the designs come frozen from visual-atelier, designs.ts).
// The page is the history: every year, and under a year every run ever written for it (a year done again — 补做 — is
// one more run, older ones stay; each says which design it was made for and is always drawn with that one).
// 翻开 opens the 小报 over the desk (Journey.tsx); 立即运行 writes this year up to today; 补做 writes a past year.
// Only this page deletes a run (Rust atlas_delete, undoable): no AI tool, no schedule can.
import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { moduleById } from "@/app/modules";
import { useTabs } from "@/app/tabs";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Play, Plus, X } from "@/shared/icons";
import { api as ipc } from "@/shared/ipc";
import { useOpenRequest, type Hit } from "@/shared/open";
import { Button, EmptyState, IconButton, PageHeader } from "@/shared/ui";
import { ATLAS_KEY, atlasApi, OVERVIEW_KEY, useAtlasOverview, type Entry } from "./api";
import { designLabel } from "./designs";
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

function Cover(props: { year: string; count: number; tilt: number }) {
  return (
    <span className="atlas-cover" style={{ "--tilt": `${props.tilt}deg` } as React.CSSProperties}>
      {props.count > 1 && <span className="atlas-cover-under" aria-hidden />}
      <span className="atlas-cover-paper" aria-hidden />
      <span className="atlas-cover-name">柚子的年度小报</span>
      <span className="atlas-cover-year">{props.year}</span>
      <span className="atlas-cover-bar" aria-hidden />
      {props.count > 1 && <span className="atlas-cover-count">{props.count} 份</span>}
    </span>
  );
}

export default function AtlasPage() {
  const m = moduleById("atlas");
  const qc = useQueryClient();
  const toast = useToast();
  const tabs = useTabs();
  const [confirm, confirmDialog] = useConfirm();
  const { data: o } = useAtlasOverview();
  const [open, setOpen] = useState<{ id: number; desk: boolean } | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
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
  }, []);

  if (!o) return <div className="flex h-full flex-col"><PageHeader icon={m.icon} color={m.color} title={m.label} /></div>;
  const thisYear = o.today.slice(0, 4);
  const byYear = new Map<string, Entry[]>();
  for (const e of o.entries) byYear.set(e.year, [...(byYear.get(e.year) ?? []), e]);
  // every year with a run or with records, newest first; this year always
  const years = [...new Set([thisYear, ...byYear.keys(), ...o.years.map((y) => y.year)])].filter((y) => y <= thisYear).sort((a, b) => b.localeCompare(a));
  const records = new Map(o.years.map((y) => [y.year, y.records]));
  const year = picked && years.includes(picked) ? picked : (years.find((y) => byYear.has(y)) ?? null);
  const runs = year ? (byYear.get(year) ?? []) : [];
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
      setPicked(y);
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
  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        icon={m.icon}
        color={m.color}
        title={m.label}
        right={
          <span className="flex items-center justify-end gap-3">
            {total > 0 && (
              <span className="pointer-events-none">
                <b className="text-base text-foreground">{byYear.size}</b> 年 · <b className="text-base text-foreground">{total}</b> 份
              </span>
            )}
            <Button variant="primary" disabled={!!o.writing} onClick={() => void run(thisYear)}>
              <Play size={14} /> 立即运行（写到今天）
            </Button>
          </span>
        }
      />
      <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
        <div className="space-y-8 px-14 pt-4 pb-10">
          {total === 0 && !o.writing && <EmptyState title="还没有年度小报" />}
          <div className="atlas-years">
            {years.map((y, i) => {
              const list = byYear.get(y) ?? [];
              const best = list.find((e) => e.scenes > 0) ?? list[0];
              const writing = o.writing === y;
              return (
                <div key={y} className={`atlas-year ${y === year ? "is-picked" : ""}`}>
                  {best ? (
                    <button type="button" className="pm-press" aria-label={`${y} 年做过的`} onClick={() => setPicked(y)}>
                      <Cover year={y} count={list.length} tilt={[-2, 1.5, -1][i % 3]} />
                    </button>
                  ) : (
                    <span className="atlas-cover is-none">{writing ? "正在写…" : "还没写"}</span>
                  )}
                  <div className="mt-3 font-bold tabular-nums">{y} 年</div>
                  <div className="text-xs whitespace-nowrap text-muted-foreground">{best ? `${span(best)} · ${told(best)}` : records.get(y) ? `有 ${records.get(y)} 条记录` : "还没有记录"}</div>
                  <div className="mt-2 flex h-9 items-center">
                    {writing ? (
                      <span className="text-sm text-muted-foreground">柚子在写…</span>
                    ) : best ? (
                      <Button onClick={() => setOpen({ id: best.id, desk: false })}>翻开</Button>
                    ) : (
                      <Button disabled={!!o.writing} onClick={() => void run(y)}>
                        {y === thisYear ? "写到今天" : "补做这一年"}
                      </Button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
          {year && runs.length > 0 && (
            <section className="rounded-card border bg-card">
              <div className="flex items-center gap-3 border-b px-4 py-2.5">
                <h2 className="font-bold tabular-nums">{year} 年做过的</h2>
                <span className="text-xs text-muted-foreground">{runs.length} 份 · 每一份都留着，照它写的时候那一版设计画</span>
                <div className="flex-1" />
                <Button variant="ghost" disabled={!!o.writing} onClick={() => void run(year)}>
                  <Plus size={14} /> {year === thisYear ? "再写一份（到今天）" : "再做一份"}
                </Button>
              </div>
              {runs.map((e) => (
                <div key={e.id} className="flex min-h-14 items-center gap-4 border-b px-4 py-2 last:border-b-0">
                  <div className="w-44 shrink-0">
                    <div className="font-bold tabular-nums">{wrote(e)}</div>
                    <div className="text-xs text-muted-foreground">
                      {span(e)} · {told(e)}
                    </div>
                  </div>
                  <div className="min-w-0 flex-1 text-xs text-muted-foreground">
                    <div className="text-sm text-foreground">{designLabel(e.design)}</div>
                    <div className="truncate">{e.error ? e.error : e.modelName}</div>
                  </div>
                  <Button onClick={() => setOpen({ id: e.id, desk: false })}>翻开</Button>
                  <IconButton label={`删除 ${wrote(e)} 写的这一份`} danger onClick={() => void remove(e)}>
                    <X size={14} />
                  </IconButton>
                </div>
              ))}
            </section>
          )}
        </div>
      </div>
      {open && <Journey key={open.id} id={open.id} desk={open.desk} onOpenRecord={openRecord} onClose={() => setOpen(null)} />}
      {confirmDialog}
    </div>
  );
}
