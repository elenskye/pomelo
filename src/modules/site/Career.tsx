// 职业页 (each language its own page): 联系邮箱, 页面开头摘要, 简历下载 (按钮文字 + a document of the library: a
// file chosen is added to it, 资源库… picks one there), 页面模块 (add, move, remove; 小标题 + 要点 or 时间 + 角色 + 机构 + 要点 — switching
// empties the module, asked first), 附加内容 (the body).
import { useState } from "react";
import { FilePlus } from "lucide-react";
import { useConfirm } from "@/shared/feedback";
import { Polish } from "@/modules/picky/Assist";
import { useSetting } from "@/shared/settings";
import { Field, Segmented, TextArea, TextInput } from "@/shared/ui";
import { SITE_KEY, pageApi, type Career, type CareerVersion, type Detailed, type Lang, type Section, type Simple } from "./api";
import { AddRow, BodyField, DocBar, RowControls, SizedInput, addTo, moved, pickFiles, replaced, useDoc, usePreview, type View } from "./parts";
import { AssetPicker } from "./WorkEditor";

const MODES: { value: Section["mode"]; label: string }[] = [
  { value: "simple", label: "小标题 + 要点" },
  { value: "detailed", label: "时间 + 角色 + 机构 + 要点" },
];
const small = "py-1 text-xs";

function Resumes(props: { lang: Lang; v: CareerVersion; set: (patch: Partial<CareerVersion>) => void; onError: (e: unknown) => void }) {
  const rows = props.v.resumes;
  const [picking, setPicking] = useState<number | "new" | null>(null);
  const use = (i: number | "new", file: string) => props.set({ resumes: i === "new" ? [...rows, { label: "", file }] : replaced(rows, i, { file }) });
  const choose = (i: number | "new") =>
    void pickFiles(false, false).then(async (paths) => {
      if (!paths.length) return;
      try {
        const [url] = await addTo(`cv:${props.lang}`)({ paths });
        use(i, `public${url}`);
      } catch (e) {
        props.onError(e);
      }
    });
  const link = "rounded px-1 text-xs text-muted-foreground hover:text-foreground";
  return (
    <div className="space-y-2">
      {rows.map((r, i) => (
        <div key={i} className="flex items-center gap-2">
          <SizedInput aria-label="按钮文字" placeholder="按钮文字" value={r.label} onChange={(e) => props.set({ resumes: replaced(rows, i, { label: e.target.value }) })} className={`w-44 shrink-0 ${small}`} />
          <button type="button" onClick={() => choose(i)} title={r.file} className="min-w-0 flex-1 truncate rounded-control border px-2 py-1 text-left font-mono text-xs text-muted-foreground hover:bg-muted">
            {r.file.split("/").pop() || "选择文件…"}
          </button>
          <button type="button" className={`shrink-0 ${link}`} onClick={() => setPicking(i)}>
            资源库…
          </button>
          <RowControls i={i} n={rows.length} onMove={(to) => props.set({ resumes: moved(rows, i, to) })} onRemove={() => props.set({ resumes: rows.filter((_, j) => j !== i) })} />
        </div>
      ))}
      <div className="flex gap-x-1">
        <button type="button" className={`inline-flex items-center gap-1 ${link}`} onClick={() => choose("new")}>
          <FilePlus size={12} strokeWidth={1.5} /> 选择…
        </button>
        <button type="button" className={link} onClick={() => setPicking("new")}>
          资源库…
        </button>
      </div>
      {picking !== null && (
        <AssetPicker
          kind="document"
          onClose={() => setPicking(null)}
          onPick={(a) => {
            use(picking, a.path);
            setPicking(null);
          }}
        />
      )}
    </div>
  );
}

function SimpleRows(props: { rows: Simple[]; set: (rows: Simple[]) => void }) {
  return (
    <div className="space-y-2">
      {props.rows.map((r, i) => (
        <div key={i} className="flex gap-2">
          <div className="min-w-0 flex-1 space-y-1">
            <TextInput aria-label="小标题" placeholder="小标题" value={r.heading} onChange={(e) => props.set(replaced(props.rows, i, { heading: e.target.value }))} className={small} />
            <TextArea aria-label="内容要点" placeholder="内容要点" rows={2} value={r.items} onChange={(e) => props.set(replaced(props.rows, i, { items: e.target.value }))} className="text-xs" />
          </div>
          <RowControls i={i} n={props.rows.length} onMove={(to) => props.set(moved(props.rows, i, to))} onRemove={() => props.set(props.rows.filter((_, j) => j !== i))} vertical />
        </div>
      ))}
      <AddRow onClick={() => props.set([...props.rows, { heading: "", items: "" }])}>加一条</AddRow>
    </div>
  );
}

function DetailedRows(props: { rows: Detailed[]; set: (rows: Detailed[]) => void }) {
  return (
    <div className="space-y-2">
      {props.rows.map((r, i) => (
        <div key={i} className="flex gap-2">
          <div className="min-w-0 flex-1 space-y-1">
            <div className="grid grid-cols-3 gap-1">
              <TextInput aria-label="时间段" placeholder="时间段" value={r.period} onChange={(e) => props.set(replaced(props.rows, i, { period: e.target.value }))} className={small} />
              <TextInput aria-label="角色 / 名称" placeholder="角色 / 名称" value={r.role} onChange={(e) => props.set(replaced(props.rows, i, { role: e.target.value }))} className={small} />
              <TextInput aria-label="机构 / 项目" placeholder="机构 / 项目" value={r.org} onChange={(e) => props.set(replaced(props.rows, i, { org: e.target.value }))} className={small} />
            </div>
            <TextArea aria-label="要点（一行一条）" placeholder="要点" rows={3} value={r.bullets.join("\n")} onChange={(e) => props.set(replaced(props.rows, i, { bullets: e.target.value.split("\n") }))} className="text-xs" />
          </div>
          <RowControls i={i} n={props.rows.length} onMove={(to) => props.set(moved(props.rows, i, to))} onRemove={() => props.set(props.rows.filter((_, j) => j !== i))} vertical />
        </div>
      ))}
      <AddRow onClick={() => props.set([...props.rows, { period: "", role: "", org: "", bullets: [] }])}>加一条</AddRow>
    </div>
  );
}

function Sections(props: { v: CareerVersion; set: (patch: Partial<CareerVersion>) => void }) {
  const [confirm, confirmDialog] = useConfirm();
  const secs = props.v.sections;
  const setSec = (i: number, patch: Partial<Section>) => props.set({ sections: replaced(secs, i, patch) });
  const switchMode = async (i: number, mode: Section["mode"]) => {
    const s = secs[i];
    if (mode === s.mode) return;
    const filled = s.mode === "simple" ? s.simple.length : s.detailed.length;
    if (filled && !(await confirm({ title: "换内容模式？", message: `这个模块的 ${filled} 条内容会清空。`, confirmLabel: "换", danger: true }))) return;
    setSec(i, { mode, simple: [], detailed: [] });
  };
  return (
    <div className="space-y-3">
      {secs.map((s, i) => (
        <div key={i} className="space-y-2 rounded-card border p-3">
          <div className="flex items-center gap-2">
            <TextInput aria-label="模块标题" placeholder="模块标题" value={s.title} onChange={(e) => setSec(i, { title: e.target.value })} className="min-w-0 flex-1 font-bold" />
            <RowControls i={i} n={secs.length} onMove={(to) => props.set({ sections: moved(secs, i, to) })} onRemove={() => props.set({ sections: secs.filter((_, j) => j !== i) })} removeLabel="删除模块" />
          </div>
          <Segmented ariaLabel="内容模式" value={s.mode} onChange={(m) => void switchMode(i, m)} options={MODES} />
          {s.mode === "simple" ? <SimpleRows rows={s.simple} set={(simple) => setSec(i, { simple })} /> : <DetailedRows rows={s.detailed} set={(detailed) => setSec(i, { detailed })} />}
        </div>
      ))}
      <AddRow onClick={() => props.set({ sections: [...secs, { title: "", mode: "detailed", simple: [], detailed: [] }] })}>加一个模块</AddRow>
      {confirmDialog}
    </div>
  );
}

export function CareerPage(props: { setDirty: (d: boolean) => void; onError: (e: unknown) => void; onChanged: () => void }) {
  const [view, setView] = useSetting<View>("ui.site_work_view", "zh", (v): v is View => v === "zh" || v === "en" || v === "both");
  const doc = useDoc<Career>([...SITE_KEY, "career"], pageApi.career, pageApi.careerSave, props.setDirty, props.onError, props.onChanged);
  const { previewing, preview } = usePreview(props.onError);
  const d = doc.draft;
  if (!d) return null;
  const langs: Lang[] = view === "both" ? ["zh", "en"] : [view];
  const setV = (l: Lang) => (patch: Partial<CareerVersion>) => doc.setDraft((x) => (x && x[l] ? { ...x, [l]: { ...x[l]!, ...patch } } : x));
  return (
    <div className="flex min-h-0 flex-1 flex-col border-t">
      <DocBar
        view={view}
        setView={setView}
        onPreview={() => void preview(view === "en" ? "/en/career/" : "/career/", doc.save)}
        previewing={previewing}
        onSave={() => void doc.save()}
        dirty={doc.dirty}
        saving={doc.saving}
        menu={[{ value: "reload", label: "重新读取" }]}
        onMenu={() => void doc.reload()}
      />
      <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
        <div className={`mx-auto grid gap-8 px-6 py-5 ${langs.length === 2 ? "max-w-[1200px] grid-cols-2" : "max-w-[760px]"}`}>
          {langs.map((l) => {
            const v = d[l];
            if (!v) return <p key={l} className="text-sm text-muted-foreground">{l === "en" ? "网站里还没有英文版的职业页" : "网站里还没有中文版的职业页"}</p>;
            return (
              <div key={l} className="space-y-5">
                <Field label="联系邮箱">
                  <TextInput value={v.email} onChange={(e) => setV(l)({ email: e.target.value })} />
                </Field>
                <Field label="页面开头摘要">
                  <Polish module="site" value={v.summary} onChange={(summary) => setV(l)({ summary })}>
                    <TextArea rows={3} value={v.summary} onChange={(e) => setV(l)({ summary: e.target.value })} className="text-[13px]" />
                  </Polish>
                </Field>
                <Field label="简历下载">
                  <Resumes lang={l} v={v} set={setV(l)} onError={props.onError} />
                </Field>
                <Field label="页面模块">
                  <Sections v={v} set={setV(l)} />
                </Field>
                <Field label="附加内容">
                  <BodyField lang={l} label="附加内容" body={v.extra} setBody={(extra) => setV(l)({ extra })} addImages={addTo("career")} onError={props.onError} />
                </Field>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
