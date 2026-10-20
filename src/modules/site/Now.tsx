// Now: 当下状态 on top (one line each in 中文 / English, its own 保存), then the months (newest first, 无英文
// marked) and the open month's text in 中文 · English · 并排 — the body only, with 润色 and 按中文起草.
import { useEffect, useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { errorText, useConfirm } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { MonthInput } from "@/shared/MonthInput";
import { useSetting } from "@/shared/settings";
import { Button, EmptyState, Field, IconButton, Segmented } from "@/shared/ui";
import { SITE_KEY, pageApi, type Lang, type NowMonth, type Status } from "./api";
import { BodyField, DocBar, DraftAction, SizedInput, addTo, shown, stored, useDoc, useEnglishDraft, usePreview, type View } from "./parts";

const NOW_KEY = [...SITE_KEY, "now"];
const monthText = (m: string) => m.replace("-", " / ");

function StatusCard(props: { status: Status; onSaved: (s: Status) => void; onError: (e: unknown) => void }) {
  const [zh, setZh] = useState(props.status.zh);
  const [en, setEn] = useState(props.status.en);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    setZh(props.status.zh);
    setEn(props.status.en);
  }, [props.status]);
  const changed = zh.trim() !== props.status.zh || en.trim() !== props.status.en;
  const save = async () => {
    setSaving(true);
    try {
      props.onSaved(await pageApi.statusSave({ ...props.status, zh, en }));
    } catch (e) {
      props.onError(e);
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="flex flex-wrap items-end gap-3 px-6 py-3">
      <Field label="当下状态（中文）">
        <SizedInput value={zh} onChange={(e) => setZh(e.target.value)} className="w-56" />
      </Field>
      <Field label="当下状态（English）">
        <SizedInput value={en} onChange={(e) => setEn(e.target.value)} className="w-56" />
      </Field>
      <Button disabled={!changed || saving} onClick={() => void save()}>
        {saving ? "正在保存…" : "保存"}
      </Button>
    </div>
  );
}

function NewMonth(props: { onClose: () => void; onCreated: (m: string) => void }) {
  const [month, setMonth] = useState<string | null>(new Date().toISOString().slice(0, 7));
  const [lang, setLang] = useState<Lang>("zh");
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!month) return;
    try {
      props.onCreated((await pageApi.nowCreate(month, lang)).month);
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal title="新的月度记录" onClose={props.onClose} onSubmit={(e) => void submit(e)} error={error} submitLabel="新建" submitDisabled={!month}>
      <Segmented ariaLabel="语言" value={lang} onChange={setLang} options={[{ value: "zh", label: "中文" }, { value: "en", label: "English" }]} />
      <Field label="年月">
        <MonthInput value={month} onChange={setMonth} />
      </Field>
    </Modal>
  );
}

function MonthEditor(props: { month: string; view: View; setView: (v: View) => void; setDirty: (d: boolean) => void; onChanged: (m?: string | null) => void; onError: (e: unknown) => void }) {
  const [confirm, confirmDialog] = useConfirm();
  const doc = useDoc<NowMonth>([...NOW_KEY, props.month], () => pageApi.now(props.month), pageApi.nowSave, props.setDirty, props.onError, () => props.onChanged());
  const { previewing, preview } = usePreview(props.onError);
  const english = useEnglishDraft(`now:${props.month}`, (m) => doc.setDraft((d) => (d && d.en !== null && m.body ? { ...d, en: stored(shown(m.body)) } : d)), props.onError, !!doc.draft);
  const d = doc.draft;
  if (!d) return <div className="flex-1" />;
  const langs: Lang[] = props.view === "both" ? ["zh", "en"] : [props.view];
  const add = async (lang: Lang) => {
    const now = await doc.save();
    if (!now) return;
    try {
      await pageApi.nowCreate(d.month, lang);
      await doc.reload();
      props.onChanged();
    } catch (e) {
      props.onError(e);
    }
  };
  const remove = async () => {
    if (!(await confirm({ title: `删除 ${monthText(d.month)} 的记录？`, message: [d.zh !== null && "中文版", d.en !== null && "英文版"].filter(Boolean).join("和") + "一起删掉。", confirmLabel: "删除", danger: true }))) return;
    try {
      await pageApi.nowDelete(d.month, doc.saved!.prints);
      props.setDirty(false);
      props.onChanged(null);
    } catch (e) {
      props.onError(e);
    }
  };
  const startDraft = async () => {
    if (!d.zh?.trim() || d.en === null) return;
    if (d.en.trim() && !(await confirm({ title: "用按中文起草的英文替换现在的英文？", message: "保存了才写进文件。", confirmLabel: "起草" }))) return;
    await english.start({ title: "", summary: "", body: d.zh });
  };
  const lang = props.view === "en" ? "en" : "zh";
  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <DocBar
        view={props.view}
        setView={props.setView}
        left={<span className="text-sm text-muted-foreground tabular-nums">{monthText(d.month)}</span>}
        onPreview={() => void preview(lang === "en" ? "/en/now/" : "/now/", doc.save)}
        previewing={previewing}
        onSave={() => void doc.save()}
        dirty={doc.dirty}
        saving={doc.saving}
        menu={[
          { value: "reload", label: "重新读取" },
          { value: "delete", label: <span className="text-destructive">删除这个月…</span> },
        ]}
        onMenu={(v) => (v === "reload" ? void doc.reload() : void remove())}
      />
      <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
        <div className={`mx-auto grid gap-8 px-6 py-5 ${langs.length === 2 ? "max-w-[1200px] grid-cols-2" : "max-w-[760px]"}`}>
          {langs.map((l) => {
            const body = d[l];
            if (body === null)
              return (
                <div key={l} className="space-y-3 rounded-card border bg-card p-4">
                  <p className="text-sm font-bold">{l === "en" ? "这个月还没有英文版" : "这个月还没有中文版"}</p>
                  <Button variant="primary" onClick={() => void add(l)}>
                    {l === "en" ? "新建英文版" : "新建中文版"}
                  </Button>
                </div>
              );
            const locked = l === "en" && english.drafting;
            return (
              <BodyField
                key={l}
                lang={l}
                label="正文"
                body={body}
                setBody={(b) => doc.setDraft((x) => (x ? { ...x, [l]: b } : x))}
                addImages={addTo("now")}
                onError={props.onError}
                locked={locked}
                note={locked ? english.mine?.note : null}
                draft={l === "en" && d.zh?.trim() ? <DraftAction job={english.job} mine={english.mine} onStart={() => void startDraft()} onStop={english.stop} /> : undefined}
              />
            );
          })}
        </div>
      </div>
      {confirmDialog}
    </div>
  );
}

export function NowPage(props: { guard: { confirmLeave: () => Promise<boolean>; setDirty: (d: boolean) => void }; onError: (e: unknown) => void }) {
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: NOW_KEY, queryFn: pageApi.nowList, refetchOnWindowFocus: true });
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [view, setView] = useSetting<View>("ui.site_work_view", "zh", (v): v is View => v === "zh" || v === "en" || v === "both");
  const months = data?.months ?? [];
  useEffect(() => {
    if (data && (!selected || !months.some((m) => m.month === selected))) setSelected(months[0]?.month ?? null);
  }, [data]); // eslint-disable-line react-hooks/exhaustive-deps
  const changed = (m?: string | null) => {
    void qc.invalidateQueries({ queryKey: NOW_KEY, exact: true });
    void qc.invalidateQueries({ queryKey: [...SITE_KEY, "changes"] });
    if (m !== undefined) setSelected(m);
  };
  const open = async (m: string) => {
    if (m === selected) return;
    if (await props.guard.confirmLeave()) {
      props.guard.setDirty(false);
      setSelected(m);
    }
  };
  if (!data) return null;
  return (
    <div className="flex min-h-0 flex-1 flex-col border-t">
      <StatusCard status={data.status} onSaved={() => changed()} onError={props.onError} />
      <div className="flex min-h-0 flex-1 border-t">
        {view !== "both" && (
          <aside className="flex w-48 shrink-0 flex-col border-r">
            <div className="flex items-center justify-between px-3 py-2">
              <span className="text-xs text-muted-foreground">月度记录</span>
              <IconButton label="新的月度记录" onClick={() => void props.guard.confirmLeave().then((ok) => ok && setCreating(true))}>
                <Plus size={16} strokeWidth={1.5} />
              </IconButton>
            </div>
            <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
              <div className="space-y-0.5 px-2 pb-4">
                {months.map((m) => (
                  <button
                    key={m.month}
                    type="button"
                    onClick={() => void open(m.month)}
                    className={`flex w-full items-center justify-between gap-2 rounded-control px-3 py-2 text-left text-sm tabular-nums outline-none focus-visible:ring-2 focus-visible:ring-ring ${m.month === selected ? "bg-accent" : "hover:bg-muted/70"}`}
                  >
                    {monthText(m.month)}
                    {m.langs.length < 2 && <span className="rounded-full bg-muted px-1.5 text-[11px] text-muted-foreground">{m.langs[0] === "zh" ? "无英文" : "无中文"}</span>}
                  </button>
                ))}
              </div>
            </div>
          </aside>
        )}
        {selected ? (
          <MonthEditor key={selected} month={selected} view={view} setView={setView} setDirty={props.guard.setDirty} onChanged={changed} onError={props.onError} />
        ) : (
          <EmptyState title="还没有月度记录" />
        )}
      </div>
      {creating && (
        <NewMonth
          onClose={() => setCreating(false)}
          onCreated={(m) => {
            setCreating(false);
            props.guard.setDirty(false);
            changed(m);
          }}
        />
      )}
    </div>
  );
}
