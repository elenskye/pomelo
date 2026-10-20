// 关于我: 中文 · English · 并排 — 一点背景 (a few paragraphs), 时间轴 (rows: 职位 / 身份、公司 / 学校、时间段、一句话
// 简介), 一点故事 (the body, 润色 / 按中文起草); on the right the pictures both languages share: 肖像 (the site
// shows one at random) and 画作 / 摄影 (in order).
import { useState } from "react";
import { Polish } from "@/modules/picky/Assist";
import { useConfirm } from "@/shared/feedback";
import { useSetting } from "@/shared/settings";
import { Field, TextArea, TextInput } from "@/shared/ui";
import { SITE_KEY, pageApi, type About, type AboutVersion, type Entry, type Lang } from "./api";
import { AddRow, BodyField, DocBar, DraftAction, ImageList, RowControls, addTo, moved, replaced, shown, stored, useDoc, useEnglishDraft, usePreview, type View } from "./parts";
import { AssetPicker } from "./WorkEditor";

function Timeline(props: { rows: Entry[]; set: (rows: Entry[]) => void; locked?: boolean }) {
  return (
    <div className="space-y-2">
      {props.rows.map((e, i) => (
        <div key={i} className="flex gap-2 rounded-card border p-2">
          <div className="min-w-0 flex-1 space-y-1.5">
            <div className="grid grid-cols-3 gap-1.5">
              <TextInput aria-label="职位 / 身份" placeholder="职位 / 身份" value={e.role} readOnly={props.locked} onChange={(x) => props.set(replaced(props.rows, i, { role: x.target.value }))} className="py-1 text-xs" />
              <TextInput aria-label="公司 / 学校" placeholder="公司 / 学校" value={e.org} readOnly={props.locked} onChange={(x) => props.set(replaced(props.rows, i, { org: x.target.value }))} className="py-1 text-xs" />
              <TextInput aria-label="时间段" placeholder="时间段" value={e.period} readOnly={props.locked} onChange={(x) => props.set(replaced(props.rows, i, { period: x.target.value }))} className="py-1 text-xs" />
            </div>
            <Polish module="site" value={e.description} onChange={(t) => props.set(replaced(props.rows, i, { description: t }))}>
              <TextArea aria-label="一句话简介" placeholder="一句话简介" rows={2} value={e.description} readOnly={props.locked} onChange={(x) => props.set(replaced(props.rows, i, { description: x.target.value }))} className="text-xs" />
            </Polish>
          </div>
          {!props.locked && <RowControls i={i} n={props.rows.length} onMove={(to) => props.set(moved(props.rows, i, to))} onRemove={() => props.set(props.rows.filter((_, j) => j !== i))} vertical />}
        </div>
      ))}
      {!props.locked && <AddRow onClick={() => props.set([...props.rows, { role: "", org: "", period: "", description: "" }])}>加一行</AddRow>}
    </div>
  );
}

export function AboutPage(props: { setDirty: (d: boolean) => void; onError: (e: unknown) => void; onChanged: () => void }) {
  const [view, setView] = useSetting<View>("ui.site_work_view", "zh", (v): v is View => v === "zh" || v === "en" || v === "both");
  const [confirm, confirmDialog] = useConfirm();
  const [picking, setPicking] = useState<"portraits" | "artworks" | "body" | null>(null);
  const doc = useDoc<About>([...SITE_KEY, "about"], pageApi.about, pageApi.aboutSave, props.setDirty, props.onError, props.onChanged);
  const { previewing, preview } = usePreview(props.onError);
  const english = useEnglishDraft("about", (m) => doc.setDraft((d) => (d?.en && m.body ? { ...d, en: { ...d.en, story: stored(shown(m.body)) } } : d)), props.onError, !!doc.draft);
  const d = doc.draft;
  if (!d) return null;
  const langs: Lang[] = view === "both" ? ["zh", "en"] : [view];
  const setV = (l: Lang) => (patch: Partial<AboutVersion>) => doc.setDraft((x) => (x && x[l] ? { ...x, [l]: { ...x[l]!, ...patch } } : x));
  const setImages = (k: "portraits" | "artworks", list: string[]) => doc.setDraft((x) => (x ? { ...x, images: { ...x.images, [k]: list } } : x));
  const startDraft = async () => {
    if (!d.zh?.story.trim() || !d.en) return;
    if (d.en.story.trim() && !(await confirm({ title: "用按中文起草的英文替换现在的「一点故事」？", message: "保存了才写进文件。", confirmLabel: "起草" }))) return;
    await english.start({ title: "", summary: "", body: d.zh.story });
  };
  return (
    <div className="flex min-h-0 flex-1 flex-col border-t">
      <DocBar
        view={view}
        setView={setView}
        onPreview={() => void preview(view === "en" ? "/en/about/" : "/about/", doc.save)}
        previewing={previewing}
        onSave={() => void doc.save()}
        dirty={doc.dirty}
        saving={doc.saving}
        menu={[{ value: "reload", label: "重新读取" }]}
        onMenu={() => void doc.reload()}
      />
      <div className="flex min-h-0 flex-1">
        <div className="-m-1 min-w-0 flex-1 overflow-y-auto p-1">
          <div className={`mx-auto grid gap-8 px-6 py-5 ${langs.length === 2 ? "max-w-[1200px] grid-cols-2" : "max-w-[760px]"}`}>
            {langs.map((l) => {
              const v = d[l];
              if (!v) return <p key={l} className="text-sm text-muted-foreground">{l === "en" ? "网站里还没有英文版的关于我" : "网站里还没有中文版的关于我"}</p>;
              const locked = l === "en" && english.drafting;
              return (
                <div key={l} className="space-y-5">
                  <Field label="一点背景">
                    <Polish module="site" value={v.contextIntro} onChange={(t) => setV(l)({ contextIntro: t })}>
                      <TextArea rows={4} value={v.contextIntro} readOnly={locked} onChange={(e) => setV(l)({ contextIntro: e.target.value })} className="text-[13px]" />
                    </Polish>
                  </Field>
                  <Field label="时间轴">
                    <Timeline rows={v.timeline} set={(timeline) => setV(l)({ timeline })} locked={locked} />
                  </Field>
                  <Field label="一点故事">
                    <BodyField
                      lang={l}
                      label="一点故事"
                      body={v.story}
                      setBody={(story) => setV(l)({ story })}
                      addImages={addTo("about")}
                      onError={props.onError}
                      locked={locked}
                      note={locked ? english.mine?.note : null}
                      draft={l === "en" && d.zh?.story.trim() ? <DraftAction job={english.job} mine={english.mine} onStart={() => void startDraft()} onStop={english.stop} /> : undefined}
                    />
                  </Field>
                </div>
              );
            })}
          </div>
        </div>
        <aside className="-m-1 w-64 shrink-0 overflow-y-auto border-l p-1">
          <div className="space-y-5 px-4 py-5">
            <Field label="肖像">
              <ImageList items={d.images.portraits} onChange={(l) => setImages("portraits", l)} target="portraits" onError={props.onError} onPickAsset={() => setPicking("portraits")} />
            </Field>
            <Field label="画作 / 摄影">
              <ImageList items={d.images.artworks} onChange={(l) => setImages("artworks", l)} target="artworks" onError={props.onError} onPickAsset={() => setPicking("artworks")} />
            </Field>
          </div>
        </aside>
      </div>
      {picking && picking !== "body" && (
        <AssetPicker
          onClose={() => setPicking(null)}
          onPick={(a) => {
            setImages(picking, [...d.images[picking], a.path]);
            setPicking(null);
          }}
        />
      )}
      {confirmDialog}
    </div>
  );
}
