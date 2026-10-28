// Pub2MD (助手; rules in Rust assistants/pub2md/). Two tabs, like Translator's. Pub2MD: settings saved as
// they change (做哪几步, 输出格式, 另存为, 双语, 翻译 (语言, 不翻的部分, 联网查术语), 精修, the Worker's and the
// Supervisor's model and thinking level, the backup model, 同时几块, 存到哪), the Mathpix keys (not in the mock), files picked
// or dropped anywhere on the page (shown as the documents they will be, sent with 发送), the document at work (step, chunks, requests, time; 停止), what waits,
// and the last 10 documents (an .mmd / .md result opens in the preview, other files and the folder with
// their app; 继续 or 删除 a stopped one: its folder to the Trash), and at the bottom what it does and the model
// pairs the real tests settled on (ABOUT, PAIRS). 提示词: the user's own paragraph, 文体 and 领域, and what each model is told.
// The work goes on in Rust when the page is left: the page polls while something is at work.
import { Fragment, useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FileText, FolderOpen, X } from "lucide-react";
import { MmdPreview } from "./MmdPreview";
import { Dropdown, type DropdownOption } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { useDropTarget } from "@/shared/attachments/target";
import { Button, IconButton, Segmented, Switch, TextArea, TextInput } from "@/shared/ui";
import { pickyApi, usePickyStatus, type Level, type Pub2mdFormat, type Pub2mdPart, type Pub2mdRecent, type Pub2mdSettings, type Pub2mdStaged, type Pub2mdView, type Think } from "../api";
import { modelOptions } from "../ModelMenu";
import { useModels } from "../Schedules";
import { thinkOf } from "../Thinking";
import { useSubPage } from "@/app/subpage";
import { watchJob } from "@/app/Pomelo";

export const PUB2MD_KEY = ["picky", "pub2md"];
const EXTENSIONS = ["pdf", "docx", "doc", "png", "jpg", "jpeg", "heic", "heif", "webp", "tif", "tiff", "bmp", "gif", "md", "mmd", "txt", "markdown"];
const LANGS: { value: Pub2mdSettings["lang"]; label: string }[] = [
  { value: "enZh", label: "英文 → 简体中文" },
  { value: "zhHant", label: "简体 → 繁体" },
  { value: "hantZh", label: "繁体 → 简体" },
];
const SKIPPABLE: { value: Pub2mdPart; label: string }[] = [
  { value: "front", label: "作者和单位" },
  { value: "acknowledgments", label: "致谢" },
  { value: "references", label: "参考文献" },
  { value: "appendix", label: "附录" },
];
const FORMATS: { value: Pub2mdFormat; label: string }[] = [
  { value: "docx", label: "Word" },
  { value: "pdf", label: "PDF" },
  { value: "latex", label: "LaTeX" },
  { value: "html", label: "HTML" },
];
const previewable = (f: string) => f.endsWith(".mmd") || f.endsWith(".md");
const LEVELS: { value: Level; label: string }[] = [
  { value: "off", label: "不思考" },
  { value: "low", label: "思考 · 低" },
  { value: "medium", label: "思考 · 中" },
  { value: "high", label: "思考 · 高" },
];
const STATES: Record<Pub2mdRecent["state"], string> = { running: "进行中", done: "完成", stopped: "已停止", failed: "出错" };

function duration(s: number) {
  if (s < 60) return `${s} 秒`;
  if (s < 3600) return `${Math.floor(s / 60)} 分 ${s % 60} 秒`;
  return `${Math.floor(s / 3600)} 小时 ${Math.floor((s / 60) % 60)} 分`;
}

/** Until the queue is done: a document that stopped on an error makes the pomelo say so. */
const failKey = (r: Pub2mdRecent) => `${r.folder}|${r.at}|${r.message ?? ""}`;
const watchPub2md = (start: Pub2mdView) => {
  const seen = new Set(start.recent.filter((r) => r.state === "failed").map(failKey));
  watchJob("pub2md", async () => {
    const v = await pickyApi.pub2md();
    const failed = v.recent.find((r) => r.state === "failed" && !seen.has(failKey(r)));
    if (failed) {
      seen.add(failKey(failed));
      return `Pub2MD 没做完「${failed.name}」：${failed.message ?? "出错了"}`;
    }
    return v.current || v.queue.length ? null : "";
  });
};

export function Pub2mdPage() {
  const qc = useQueryClient();
  const toast = useToast();
  const view = useQuery({ queryKey: PUB2MD_KEY, queryFn: pickyApi.pub2md, refetchInterval: (q) => (q.state.data?.current || q.state.data?.queue.length ? 500 : false) });
  const data = view.data;
  const [settings, setSettings] = useState<Pub2mdSettings | null>(null);
  useEffect(() => {
    if (data) setSettings((x) => x ?? data.settings);
  }, [data]);
  const [tab, setTab] = useSubPage<"work" | "prompt">("pub2md");
  const [preview, setPreview] = useState<{ folder: string; file: string } | null>(null);
  const typing = useRef<ReturnType<typeof setTimeout>>(undefined);
  const set = (v: Pub2mdView) => qc.setQueryData(PUB2MD_KEY, v);
  const fail = (e: unknown) => toast(errorText(e), "error");
  /** Saved at once, or (`later`: while typing) a moment after the last change. */
  const change = (next: Pub2mdSettings, later?: boolean) => {
    setSettings(next);
    clearTimeout(typing.current);
    const save = () =>
      void pickyApi.pub2mdSettings(next).then(set, (e) => {
        fail(e);
        setSettings(data?.settings ?? null);
      });
    if (later) typing.current = setTimeout(save, 600);
    else save();
  };
  const [staged, setStaged] = useState<Pub2mdStaged[]>([]);
  const [confirm, confirmDialog] = useConfirm();
  /** Picked or dropped: shown as the documents they will be; 发送 starts them. */
  const add = (paths: string[]) => {
    if (!paths.length) return;
    const all = [...new Set([...staged.flatMap((d) => d.files), ...paths])];
    void pickyApi.pub2mdGroup(all).then(setStaged, fail);
  };
  const send = () => {
    const paths = staged.flatMap((d) => d.files);
    if (paths.length)
      void pickyApi.pub2mdAdd(paths).then((v) => {
        set(v);
        setStaged([]);
        watchPub2md(v);
      }, fail);
  };
  const remove = async (r: Pub2mdRecent) => {
    if (await confirm({ title: `删除「${r.name}」？`, message: "它的文件夹（做完的部分和「继续」要用的中间结果）会移到废纸篓。", confirmLabel: "删除", danger: true }))
      void pickyApi.pub2mdDelete(r.folder).then(set, fail);
  };
  const page = useRef<HTMLDivElement>(null);
  const dropping = useDropTarget(page, { paths: add, files: (files) => add(files.map((f) => f.name)) });

  if (!data || !settings) return <div className="flex-1" />;
  const pick = async () => {
    const p = await openDialog({ multiple: true, directory: false, title: "选择文件", filters: [{ name: "PDF、Word、图片、文本", extensions: EXTENSIONS }] });
    if (p) add(Array.isArray(p) ? p : [p]);
  };
  const folder = async () => {
    const p = await openDialog({ directory: true, multiple: false, title: "存到", defaultPath: data.folder });
    if (typeof p === "string") change({ ...settings, folder: p });
  };
  const open = (folderPath: string, file?: string) => void pickyApi.pub2mdOpen(folderPath, file).catch(fail);
  const show = (folderPath: string, file?: string) => (file && previewable(file) ? setPreview({ folder: folderPath, file }) : open(folderPath, file));
  const shown = preview && data.recent.find((r) => r.folder === preview.folder);

  return (
    <div ref={page} className={`flex min-h-0 min-w-0 flex-1 flex-col px-8 pb-1 ${dropping ? "bg-accent/40" : ""}`}>
      <div className="mx-auto w-full max-w-4xl pb-3">
        <Segmented
          ariaLabel="Pub2MD"
          value={tab}
          onChange={setTab}
          options={[
            { value: "work", label: "Pub2MD" },
            { value: "prompt", label: "提示词" },
          ]}
        />
      </div>
      {tab === "prompt" && <PromptTab data={data} settings={settings} onChange={change} />}
      <div className={`-m-1 mx-auto min-h-0 w-full max-w-4xl flex-1 flex-col gap-4 overflow-y-auto p-1 pb-6 text-sm ${tab === "work" ? "flex" : "hidden"}`}>
        <Options settings={settings} onChange={change} data={data} onFolder={() => void folder()} />
        {!data.mock && <MathpixKeys saved={data.mathpix} onView={set} />}
        <div className="flex items-center gap-3">
          <Button onClick={() => void pick()} disabled={!data.mock && !data.mathpix}>
            选择文件…
          </Button>
          <Button variant="primary" onClick={send} disabled={!staged.length || (!data.mock && !data.mathpix)}>
            发送
          </Button>
          <span className="text-muted-foreground">PDF · Word · 图片 · MD · TXT</span>
        </div>
        {staged.length > 0 && (
          <section aria-label="待发送" className="flex flex-col gap-1">
            {staged.map((d) => (
              <div key={d.files.join("|")} className="flex items-center gap-2">
                <FileText size={13} strokeWidth={1.5} className="shrink-0 text-muted-foreground" />
                <span className="truncate">{d.name}</span>
                <button type="button" aria-label="移出" title="移出" onClick={() => setStaged(staged.filter((x) => x !== d))} className="rounded-control p-1 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
                  <X size={12} />
                </button>
              </div>
            ))}
          </section>
        )}
        {data.current && <Current data={data} onStop={() => void pickyApi.pub2mdStop().then(set, fail)} onOpen={open} />}
        {data.queue.length > 0 && (
          <section aria-label="等着的" className="flex flex-col gap-1">
            {data.queue.map((name, i) => (
              <div key={`${i}-${name}`} className="flex items-center gap-2 text-muted-foreground">
                <span className="truncate">等着：{name}</span>
                <button type="button" aria-label="不做了" title="不做了" onClick={() => void pickyApi.pub2mdUnqueue(i).then(set, fail)} className="rounded-control p-1 outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
                  <X size={12} />
                </button>
              </div>
            ))}
          </section>
        )}
        <Recent data={data} onOpen={show} onResume={(f) => void pickyApi.pub2mdResume(f).then((v) => (set(v), watchPub2md(v)), fail)} onDelete={(r) => void remove(r)} />
        <About />
      </div>
      {confirmDialog}
      {preview && shown && <PreviewDialog recent={shown} file={preview.file} onFile={(file) => setPreview({ ...preview, file })} onOpen={open} onClose={() => setPreview(null)} />}
    </div>
  );
}

function LevelPicker(props: { label: string; value: Level; ability: Think; onChange: (v: Level) => void }) {
  const options = props.ability === "never" ? LEVELS.slice(0, 1) : props.ability === "always" ? LEVELS.slice(1) : LEVELS;
  const value = props.ability === "never" ? "off" : props.ability === "always" && props.value === "off" ? "high" : props.value;
  return <Dropdown<Level> ariaLabel={props.label} value={value} options={options} onChange={props.onChange} />;
}

function ModelRow(props: { name: string; model: string | null; level: Level; onChange: (model: string | null, level: Level) => void }) {
  const status = usePickyStatus().data;
  const { models, keep } = useModels(props.model, props.level !== "off");
  const ability = thinkOf(status, props.model ?? status?.selection ?? null);
  return (
    <div className="flex items-center gap-2">
      <span className="w-20 shrink-0 text-muted-foreground">{props.name}</span>
      <Dropdown<string | null> ariaLabel={`${props.name} 的模型`} value={props.model} options={models} foldKey="models" onChange={(m) => props.onChange(m, props.level)} menuMaxHeight={360} />
      <LevelPicker label={`${props.name} 的思考`} value={props.level} ability={ability} onChange={(l) => props.onChange(keep(props.model, l !== "off"), l)} />
    </div>
  );
}

/** The backup model: a cloud one (a local one can't see the pages and would have to be started), or 不用. */
function BackupRow(props: { value: string; onChange: (v: string) => void }) {
  const status = usePickyStatus().data;
  const options: DropdownOption<string>[] = [...modelOptions(status).filter((o) => o.value.startsWith("cloud:")), { value: "", label: "不用" }];
  if (!options.some((o) => o.value === props.value)) options.unshift({ value: props.value, label: "（已经不在了）" });
  return (
    <div className="flex items-center gap-2">
      <span className="w-20 shrink-0 text-muted-foreground">备用模型</span>
      <Dropdown<string> ariaLabel="备用模型" value={props.value} options={options} foldKey="models" onChange={props.onChange} menuMaxHeight={360} />
    </div>
  );
}

function Options(props: { settings: Pub2mdSettings; onChange: (s: Pub2mdSettings) => void; data: Pub2mdView; onFolder: () => void }) {
  const s = props.settings;
  const clean = s.steps !== "extract";
  const translate = s.steps === "translate";
  const web = s.web ?? props.data.webDefault;
  return (
    <section aria-label="设置" className="flex flex-col gap-2.5">
      <div className="flex flex-wrap items-center gap-2">
        <Segmented
          ariaLabel="做哪几步"
          value={s.steps}
          onChange={(steps) => props.onChange({ ...s, steps })}
          options={[
            { value: "extract", label: "只提取" },
            { value: "clean", label: "整理" },
            { value: "translate", label: "整理 + 翻译" },
          ]}
        />
        {clean && (
          <Segmented
            ariaLabel="输出格式"
            value={s.md ? "md" : "mmd"}
            onChange={(v) => props.onChange({ ...s, md: v === "md" })}
            options={[
              { value: "mmd", label: "MMD" },
              { value: "md", label: "Markdown" },
            ]}
          />
        )}
        <Dropdown<Pub2mdFormat>
          ariaLabel="另存为"
          value={"" as Pub2mdFormat}
          options={FORMATS}
          isPicked={(f) => s.formats.includes(f)}
          onChange={(f) => props.onChange({ ...s, formats: s.formats.includes(f) ? s.formats.filter((x) => x !== f) : [...s.formats, f] })}
          renderTrigger={() => <span>{s.formats.length ? `另存为：${FORMATS.filter((x) => s.formats.includes(x.value)).map((x) => x.label).join("、")}` : "不另存"}</span>}
        />
        {clean && (
          <>
            <Dropdown<boolean>
              ariaLabel="中英对照的内容"
              value={s.englishOnly}
              onChange={(englishOnly) => props.onChange({ ...s, englishOnly })}
              options={[
                { value: false, label: "中英对照：都留" },
                { value: true, label: "中英对照：只留英文" },
              ]}
            />
            <Dropdown<number>
              ariaLabel="同时几块"
              value={s.parallel}
              onChange={(parallel) => props.onChange({ ...s, parallel })}
              options={[1, 2, 3, 4, 5, 6, 7, 8].map((n) => ({ value: n, label: `同时 ${n} 块` }))}
            />
          </>
        )}
      </div>
      {translate && (
        <div className="flex flex-wrap items-center gap-2">
          <Dropdown<Pub2mdSettings["lang"]> ariaLabel="翻译的语言" value={s.lang} options={LANGS} onChange={(lang) => props.onChange({ ...s, lang })} />
          <Dropdown<Pub2mdPart>
            ariaLabel="不翻的部分"
            value={"" as Pub2mdPart}
            options={SKIPPABLE}
            isPicked={(p) => s.skip.includes(p)}
            onChange={(p) => props.onChange({ ...s, skip: s.skip.includes(p) ? s.skip.filter((x) => x !== p) : [...s.skip, p] })}
            renderTrigger={() => <span>{s.skip.length ? `不翻：${SKIPPABLE.filter((x) => s.skip.includes(x.value)).map((x) => x.label).join("、")}` : "全都翻"}</span>}
          />
        </div>
      )}
      {clean && (
        <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
          {translate && s.lang === "enZh" && (
            <label className="flex items-center gap-2">
              <Switch label="联网查术语" checked={web} onChange={(v) => props.onChange({ ...s, web: v })} />
              联网查术语
            </label>
          )}
          <label className="flex items-center gap-2">
            <Switch label="精修" checked={s.refine} onChange={(refine) => props.onChange({ ...s, refine })} />
            精修
          </label>
        </div>
      )}
      {clean && (
        <>
          <ModelRow name="Worker" model={s.worker} level={s.workerLevel} onChange={(worker, workerLevel) => props.onChange({ ...s, worker, workerLevel })} />
          <ModelRow name="Supervisor" model={s.supervisor} level={s.supervisorLevel} onChange={(supervisor, supervisorLevel) => props.onChange({ ...s, supervisor, supervisorLevel })} />
          <BackupRow value={s.backup} onChange={(backup) => props.onChange({ ...s, backup })} />
        </>
      )}
      <div className="flex min-w-0 items-center gap-2">
        <span className="w-20 shrink-0 text-muted-foreground">存到</span>
        <span className="min-w-0 truncate font-mono text-xs" title={props.data.folder}>
          {props.data.folder}
        </span>
        <Button onClick={props.onFolder}>更改…</Button>
      </div>
    </section>
  );
}

function MathpixKeys(props: { saved: boolean; onView: (v: Pub2mdView) => void }) {
  const toast = useToast();
  const [editing, setEditing] = useState(false);
  const [id, setId] = useState("");
  const [key, setKey] = useState("");
  if (props.saved && !editing)
    return (
      <div className="flex items-center gap-2">
        <span className="w-20 shrink-0 text-muted-foreground">Mathpix</span>
        <span>Key 已保存</span>
        <Button onClick={() => setEditing(true)}>更新</Button>
        <Button onClick={() => void pickyApi.pub2mdKeysRemove().then(props.onView, (e) => toast(errorText(e), "error"))}>移除</Button>
      </div>
    );
  const save = () =>
    void pickyApi.pub2mdKeysSave(id, key).then(
      (v) => {
        props.onView(v);
        setId("");
        setKey("");
        setEditing(false);
        toast("已保存 Mathpix 的 Key");
      },
      (e) => toast(errorText(e), "error"),
    );
  return (
    <form
      className="flex flex-wrap items-center gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <span className="w-20 shrink-0 text-muted-foreground">Mathpix</span>
      <TextInput aria-label="Mathpix App ID" placeholder="App ID" value={id} onChange={(e) => setId(e.target.value)} className="max-w-48 font-mono" />
      <TextInput type="password" aria-label="Mathpix App Key" placeholder="App Key" value={key} onChange={(e) => setKey(e.target.value)} className="max-w-64 font-mono" />
      <Button type="submit" variant="primary" disabled={!id.trim() || !key.trim()}>
        保存
      </Button>
      {editing && <Button onClick={() => setEditing(false)}>取消</Button>}
    </form>
  );
}

function Current(props: { data: Pub2mdView; onStop: () => void; onOpen: (folder: string, file?: string) => void }) {
  const c = props.data.current!;
  const percent = c.chunks ? Math.round((c.done / c.chunks) * 100) : 0;
  const requests = c.workerRequests + c.supervisorRequests;
  return (
    <section aria-label="正在做的" className="flex flex-col gap-2 rounded-control border bg-card p-3">
      <div className="flex items-center gap-2">
        <span className="min-w-0 truncate font-medium">{c.name}</span>
        <span className="shrink-0 animate-pulse text-muted-foreground">{c.step}</span>
        <div className="flex-1" />
        {c.folder && (
          <button type="button" aria-label="打开文件夹" title="打开文件夹" onClick={() => props.onOpen(c.folder)} className="rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
            <FolderOpen size={14} strokeWidth={1.5} />
          </button>
        )}
        <Button onClick={props.onStop}>停止</Button>
      </div>
      {c.chunks > 0 && (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted" role="progressbar" aria-valuemin={0} aria-valuemax={c.chunks} aria-valuenow={c.done}>
            <div className="h-full bg-primary transition-all" style={{ width: `${percent}%` }} />
          </div>
          <span className="shrink-0">
            {c.done} / {c.chunks} 块
          </span>
        </div>
      )}
      <div className="flex flex-wrap gap-x-3 text-xs text-muted-foreground">
        {requests > 0 && (
          <span>
            请求 {requests} 次（Worker {c.workerRequests} · Supervisor {c.supervisorRequests}）
          </span>
        )}
        <span>{duration(c.elapsed)}</span>
        {c.models && <span className="truncate">{c.models}</span>}
      </div>
    </section>
  );
}

const ABOUT = "PDF、Word、图片经 Mathpix 识别，分块整理、翻译成中文。Worker 逐块做；Supervisor 审定切块、定术语表、排查出错的块、通读全文，开精修时再逐块审一遍。";

/** [用途, 搭配, 测出来的情况] — from the real tests (~/Downloads/Pub2MD 测试/测试记录.md). */
const PAIRS: [string, string, string][] = [
  ["日常", "GPT-6.1 Sol（思考 中）+ DeepSeek Flash", "35 页论文约 4 分钟、0.5 美元"],
  ["Claude", "Claude Sonnet 5.5（思考 高）+ DeepSeek Flash", "Sonnet 5.5 要开高思考，通读才够细；比 Sol 贵约一半"],
  ["精修", "交作业、交稿时开", "多改出漏译和意思偏差，费用约翻倍；日常阅读不必开"],
  ["短论文", "Sol + GLM-5.3 FlashX（思考选「总是」）", "公式编号补得最全；长文慢、贵"],
  ["备用模型", "DeepSeek Flash", "被内容审核拦下或请求一直失败时接手"],
];

function About() {
  return (
    <section aria-label="关于 Pub2MD" className="flex flex-col gap-2 border-t pt-4 text-xs text-muted-foreground">
      <p>{ABOUT}</p>
      <div className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
        {PAIRS.map(([use, pair, note]) => (
          <Fragment key={use}>
            <span>{use}</span>
            <span>
              <span className="text-foreground">{pair}</span> · {note}
            </span>
          </Fragment>
        ))}
      </div>
    </section>
  );
}

function Recent(props: { data: Pub2mdView; onOpen: (folder: string, file?: string) => void; onResume: (folder: string) => void; onDelete: (r: Pub2mdRecent) => void }) {
  const items = props.data.recent.filter((r) => r.folder !== props.data.current?.folder);
  if (!items.length) return null;
  return (
    <section aria-label="最近的" className="flex flex-col gap-2 border-t pt-4">
      {items.map((r) => (
        <div key={r.folder} className="flex flex-col gap-1">
          <div className="flex items-center gap-2">
            <span className="min-w-0 truncate">{r.name}</span>
            <span className="shrink-0 text-xs text-muted-foreground">{r.at}</span>
            <span className={`shrink-0 text-xs ${r.state === "failed" ? "text-destructive" : "text-muted-foreground"}`}>{STATES[r.state]}</span>
            <div className="flex-1" />
            {(r.state === "stopped" || r.state === "failed") && (
              <>
                <Button onClick={() => props.onResume(r.folder)}>继续</Button>
                <Button onClick={() => props.onDelete(r)}>删除</Button>
              </>
            )}
            <button type="button" aria-label="打开文件夹" title="打开文件夹" onClick={() => props.onOpen(r.folder)} className="rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
              <FolderOpen size={14} strokeWidth={1.5} />
            </button>
          </div>
          {r.message && r.state !== "done" && <span className="text-xs text-muted-foreground">{r.message}</span>}
          {r.state === "done" && r.outputs.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {r.outputs.map((f) => (
                <button key={f} type="button" onClick={() => props.onOpen(r.folder, f)} className="rounded-control border px-2 py-0.5 text-xs outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring">
                  {f}
                </button>
              ))}
            </div>
          )}
        </div>
      ))}
    </section>
  );
}

function PromptTab(props: { data: Pub2mdView; settings: Pub2mdSettings; onChange: (s: Pub2mdSettings, later?: boolean) => void }) {
  const s = props.settings;
  const told = useQuery({ queryKey: ["picky", "pub2md-prompts", JSON.stringify({ ...s, folder: null })], queryFn: () => pickyApi.pub2mdPrompts(s) });
  // 文体: one; 领域: several (an interdisciplinary text), none = 通用
  const rows: { name: string; many: boolean; choices: [string, string][]; on: (key: string) => boolean; pick: (key: string) => Pub2mdSettings }[] = [
    { name: "文体", many: false, choices: props.data.styles, on: (k) => s.style === k, pick: (k) => ({ ...s, style: k }) },
    {
      name: "领域",
      many: true,
      choices: props.data.domains,
      on: (k) => (k === "general" ? s.domains.length === 0 : s.domains.includes(k)),
      pick: (k) => ({ ...s, domains: k === "general" ? [] : s.domains.includes(k) ? s.domains.filter((d) => d !== k) : [...s.domains, k] }),
    },
  ];
  return (
    <div className="-m-1 mx-auto flex min-h-0 w-full max-w-4xl flex-1 flex-col gap-4 overflow-y-auto p-1 pb-6 text-sm">
      <div className="flex flex-col gap-1.5">
        <span>自己写的要求</span>
        <TextArea aria-label="自己写的要求" rows={5} maxLength={2000} value={s.prompt} onChange={(e) => props.onChange({ ...s, prompt: e.target.value }, true)} className="font-reading" />
      </div>
      <div className="flex flex-col gap-2.5">
        {rows.map((g) => (
          <div key={g.name} className="flex items-center gap-3">
            <span className="w-20 shrink-0 text-muted-foreground">{g.name}</span>
            <div role={g.many ? "group" : "radiogroup"} aria-label={g.name} className="flex flex-wrap gap-1.5">
              {g.choices.map(([key, label]) => {
                const on = g.on(key);
                return (
                  <button
                    key={key}
                    type="button"
                    role={g.many ? "checkbox" : "radio"}
                    aria-checked={on}
                    onClick={() => props.onChange(g.pick(key))}
                    className={`rounded-control border px-2.5 py-1 outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "border-primary bg-accent text-accent-foreground" : "hover:bg-muted"}`}
                  >
                    {label}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>
      {told.data && (
        <div className="flex flex-col gap-4 border-t pt-4">
          <span className="text-muted-foreground">发给模型的说明</span>
          {told.data.length === 0 && <p className="text-muted-foreground">只提取不用模型。</p>}
          {told.data.map(([who, text]) => (
            <section key={who} aria-label={who} className="flex flex-col gap-1">
              <span className="text-xs font-medium">{who}</span>
              <p className="selectable font-reading leading-relaxed whitespace-pre-wrap text-muted-foreground">{text}</p>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

function PreviewDialog(props: { recent: Pub2mdRecent; file: string; onFile: (f: string) => void; onOpen: (folder: string, file?: string) => void; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const toast = useToast();
  const files = props.recent.outputs.filter(previewable);
  const text = useQuery({ queryKey: ["picky", "pub2md-preview", props.recent.folder, props.file], queryFn: () => pickyApi.pub2mdPreview(props.recent.folder, props.file), staleTime: Infinity, gcTime: 60_000 });
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  useEffect(() => {
    if (text.error) toast(errorText(text.error), "error");
  }, [text.error, toast]);
  return (
    <dialog
      ref={ref}
      aria-label="预览"
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className="m-auto flex h-[86vh] w-[900px] max-w-[94vw] flex-col rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <div className="flex items-center gap-3 border-b px-5 py-3">
        <p className="min-w-0 shrink truncate text-base font-bold">{props.recent.name}</p>
        <div className="min-w-0 overflow-x-auto">
          <Segmented ariaLabel="预览哪个文件" value={props.file} onChange={props.onFile} options={files.map((f) => ({ value: f, label: f.replace(/\.(mmd|md)$/, "") }))} />
        </div>
        <div className="flex-1" />
        <Button onClick={() => props.onOpen(props.recent.folder, props.file)}>用默认程序打开</Button>
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      <div className="min-h-0 flex-1">{text.data != null ? <MmdPreview text={text.data} title={props.file} /> : <p className="p-5 text-sm text-muted-foreground">{text.error ? errorText(text.error) : "正在读取…"}</p>}</div>
    </dialog>
  );
}
