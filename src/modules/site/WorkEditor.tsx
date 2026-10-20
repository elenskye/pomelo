// One work open: 中文 · English · 并排 in the middle (title, summary, the body drawn while typing), the shared
// fields on the right (类别、话题、年月、封面、画廊、slug). Saving (保存 / ⌘S) writes both files through Rust,
// which checks they weren't changed elsewhere meanwhile. 预览 saves first, then opens the page on the
// local site in Pomelo's preview window. Images (dropped, pasted, chosen, or reused from 资源库) are
// copied into the work's own image folder by Rust. 小拾 helps (gone while it is hidden): 润色 the summaries,
// image descriptions and the body (the selection, else all of it), 补全 the English stub's title and
// summary, and 按中文起草 the English version with the 翻译 assistant's settings (Rust keeps the draft in
// memory; this page takes it in while it is written, read-only meanwhile, then clears it).
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { AssistAction, Polish, useAssistOn, useFilled } from "@/modules/picky/Assist";
import { ArrowDown, ArrowUp, FileText, ImagePlus, MoreHorizontal, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { MonthInput } from "@/shared/MonthInput";
import { useDropTarget } from "@/shared/attachments/target";
import { Button, Field, IconButton, Segmented, TextArea, TextInput } from "@/shared/ui";
import { type MdEditorHandle } from "./MdEditor";
import { BodyField, DraftAction, GHOST, LABEL, addTo, pickFiles as pickImages, shown, stored, useEnglishDraft, type View } from "./parts";
import { SITE_KEY, pagePath, siteAssist, siteSrc, useAssets, workApi, type Asset, type AssetKind, type GalleryItem, type Lang, type Taxonomy, type Version, type Work } from "./api";

export type { View };

// ---------- 资源库 ----------

export function AssetPicker(props: { kind?: AssetKind; onClose: () => void; onPick: (a: Asset) => void }) {
  const kind = props.kind ?? "image";
  const { data: assets } = useAssets(true, kind);
  const empty = assets && assets.length === 0 && <p className="py-6 text-center text-sm text-muted-foreground">{kind === "image" ? "资源库里还没有图片" : "资源库里还没有文档"}</p>;
  return (
    <Modal title={kind === "image" ? "资源库 · 图片" : "资源库 · 文档"} wide onClose={props.onClose} onSubmit={(e) => (e.preventDefault(), props.onClose())} error={null} submitLabel="关闭">
      <div className="-m-1 max-h-[55vh] overflow-y-auto p-1">
        {empty}
        {kind === "document" ? (
          !!assets?.length && <div className="overflow-hidden rounded-card border">
            {assets?.map((a) => (
              <button key={a.slug} type="button" onClick={() => props.onPick(a)} className="flex w-full items-center gap-3 border-b px-3 py-2 text-left text-sm outline-none last:border-b-0 hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring">
                <FileText size={14} className="shrink-0 text-muted-foreground" />
                <span className="font-mono text-xs">{a.slug}</span>
                <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{a.alt || a.usage}</span>
              </button>
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-3 gap-3">
            {assets?.map((a) => (
              <button key={a.slug} type="button" onClick={() => props.onPick(a)} className="group space-y-1 rounded-control p-1 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring">
                <img src={siteSrc(a.path)} alt={a.alt} className="aspect-[4/3] w-full rounded-control border bg-muted object-cover" />
                <span className="block truncate text-xs">{a.title}</span>
                {a.usage && <span className="block truncate text-[11px] text-muted-foreground">{a.usage}</span>}
              </button>
            ))}
          </div>
        )}
      </div>
    </Modal>
  );
}

// ---------- 画廊 ----------

function GalleryDialog(props: { work: Work; onClose: () => void; onDone: (g: GalleryItem[]) => void; addImages: (take: { paths?: string[]; files?: File[] }) => Promise<string[]>; onError: (e: unknown) => void }) {
  const [items, setItems] = useState<GalleryItem[]>(props.work.gallery);
  const [picking, setPicking] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const langs = (["zh", "en"] as const).filter((l) => props.work[l]);
  const add = (urls: string[]) => setItems((xs) => [...xs, ...urls.map((u) => ({ image: `public${u}`, altZh: "", captionZh: "", altEn: "", captionEn: "" }))]);
  const take = (p: Promise<string[]>) => void p.then(add, props.onError);
  const dropping = useDropTarget(box, { paths: (paths) => take(props.addImages({ paths })), files: (files) => take(props.addImages({ files })) });
  const set = (i: number, patch: Partial<GalleryItem>) => setItems((xs) => xs.map((x, j) => (j === i ? { ...x, ...patch } : x)));
  const move = (i: number, d: number) =>
    setItems((xs) => {
      const ys = [...xs];
      const [x] = ys.splice(i, 1);
      ys.splice(i + d, 0, x);
      return ys;
    });
  return (
    <Modal
      title={`画廊 · ${items.length} 张`}
      wide
      onClose={props.onClose}
      onSubmit={(e) => {
        e.preventDefault();
        props.onDone(items);
      }}
      error={null}
      submitLabel="完成"
      footerLeft={
        <>
          <Button onClick={() => void pickImages(true).then((paths) => paths.length && take(props.addImages({ paths })))}>
            <ImagePlus size={14} strokeWidth={1.5} /> 添加图片…
          </Button>
          <Button onClick={() => setPicking(true)}>资源库…</Button>
        </>
      }
    >
      <div ref={box} className={`-m-1 max-h-[58vh] overflow-y-auto rounded-control p-1 ${dropping ? "ring-2 ring-primary" : ""}`}>
        {items.length === 0 && <p className="py-8 text-center text-sm text-muted-foreground">画廊还空着</p>}
        <div className="space-y-3">
          {items.map((g, i) => (
            <div key={`${g.image}-${i}`} className="flex gap-3 rounded-card border p-2">
              <img src={siteSrc(g.image)} alt="" className="size-24 shrink-0 rounded-control bg-muted object-cover" />
              <div className="min-w-0 flex-1 space-y-1.5">
                {langs.map((l) => (
                  <div key={l} className="grid grid-cols-[3.5rem_1fr] items-center gap-x-2 gap-y-1">
                    <span className="text-xs text-muted-foreground">{LABEL[l]}</span>
                    <Polish module="site" value={l === "zh" ? g.altZh : g.altEn} onChange={(t) => set(i, l === "zh" ? { altZh: t } : { altEn: t })}>
                      <TextInput aria-label={`图片描述（${LABEL[l]}）`} placeholder="图片描述" value={l === "zh" ? g.altZh : g.altEn} onChange={(e) => set(i, l === "zh" ? { altZh: e.target.value } : { altEn: e.target.value })} className="py-1 text-xs" />
                    </Polish>
                    <span />
                    <TextInput aria-label={`图注（${LABEL[l]}）`} placeholder="图注" value={l === "zh" ? g.captionZh : g.captionEn} onChange={(e) => set(i, l === "zh" ? { captionZh: e.target.value } : { captionEn: e.target.value })} className="py-1 text-xs" />
                  </div>
                ))}
              </div>
              <div className="flex shrink-0 flex-col">
                <IconButton label="上移" disabled={i === 0} onClick={() => move(i, -1)}>
                  <ArrowUp size={14} />
                </IconButton>
                <IconButton label="下移" disabled={i === items.length - 1} onClick={() => move(i, 1)}>
                  <ArrowDown size={14} />
                </IconButton>
                <IconButton label="移出画廊" danger onClick={() => setItems((xs) => xs.filter((_, j) => j !== i))}>
                  <X size={14} />
                </IconButton>
              </div>
            </div>
          ))}
        </div>
      </div>
      {picking && (
        <AssetPicker
          onClose={() => setPicking(false)}
          onPick={(a) => {
            setPicking(false);
            setItems((xs) => [...xs, { image: a.path, altZh: props.work.zh ? a.alt : "", captionZh: "", altEn: props.work.zh ? "" : a.alt, captionEn: "" }]);
          }}
        />
      )}
    </Modal>
  );
}

// ---------- 改 slug ----------

function RenameDialog(props: { slug: string; onClose: () => void; onRename: (to: string) => Promise<void> }) {
  const [to, setTo] = useState(props.slug);
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await props.onRename(to.trim());
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal title="改 slug" onClose={props.onClose} onSubmit={(e) => void submit(e)} error={error} submitLabel="改名" submitDisabled={!to.trim() || to.trim() === props.slug}>
      <Field label="slug（网址里的名字）">
        <TextInput autoFocus value={to} onChange={(e) => setTo(e.target.value.toLowerCase())} className="font-mono" />
      </Field>
    </Modal>
  );
}

// ---------- the right column ----------

function Cover(props: { work: Work; set: (cover: string | null) => void; addImages: (take: { paths?: string[]; files?: File[] }) => Promise<string[]>; onError: (e: unknown) => void; onPickAsset: () => void }) {
  const box = useRef<HTMLDivElement>(null);
  const take = (p: Promise<string[]>) => void p.then((urls) => urls[0] && props.set(`public${urls[0]}`), props.onError);
  const dropping = useDropTarget(box, { paths: (paths) => take(props.addImages({ paths: paths.slice(0, 1) })), files: (files) => take(props.addImages({ files: files.slice(0, 1) })) });
  const cover = props.work.cover;
  return (
    <div className="space-y-1.5">
      <div ref={box} className={`overflow-hidden rounded-control border ${dropping ? "ring-2 ring-primary" : ""}`}>
        {cover ? <img src={siteSrc(cover)} alt="封面" className="aspect-[16/10] w-full bg-muted object-cover" /> : <div className="flex aspect-[16/10] items-center justify-center border-dashed text-xs text-muted-foreground">没有封面</div>}
      </div>
      <div className="flex flex-wrap gap-x-1 text-xs">
        <button type="button" className="rounded px-1 text-muted-foreground hover:text-foreground" onClick={() => void pickImages(false).then((paths) => paths.length && take(props.addImages({ paths })))}>
          选择…
        </button>
        <button type="button" className="rounded px-1 text-muted-foreground hover:text-foreground" onClick={props.onPickAsset}>
          资源库…
        </button>
        {cover && (
          <button type="button" className="rounded px-1 text-muted-foreground hover:text-destructive" onClick={() => props.set(null)}>
            移除
          </button>
        )}
      </div>
    </div>
  );
}

function SharedFields(props: { work: Work; tax: Taxonomy; set: (patch: Partial<Work>) => void; addImages: (take: { paths?: string[]; files?: File[] }) => Promise<string[]>; onError: (e: unknown) => void; onGallery: () => void; onRename: () => void; onPickAsset: () => void }) {
  const { work: w, tax } = props;
  const known = new Set(tax.topics.map((t) => t.slug));
  const topics = [...tax.topics.map((t) => ({ slug: t.slug, name: t.zh })), ...w.topics.filter((t) => !known.has(t)).map((t) => ({ slug: t, name: t }))];
  const toggle = (slug: string) => props.set({ topics: w.topics.includes(slug) ? w.topics.filter((t) => t !== slug) : [...w.topics, slug] });
  return (
    <div className="space-y-5">
      <Field label="类别">
        <Dropdown ariaLabel="类别" value={w.category} onChange={(category) => props.set({ category })} options={[{ value: "", label: "未分类" }, ...tax.categories.map((c) => ({ value: c.slug, label: `${c.zh} · ${c.en}` }))]} />
      </Field>
      <Field label="话题">
        <div className="flex flex-wrap gap-1.5">
          {topics.map((t) => {
            const on = w.topics.includes(t.slug);
            return (
              <button
                key={t.slug}
                type="button"
                aria-pressed={on}
                onClick={() => toggle(t.slug)}
                className={`rounded-full border px-2 py-0.5 text-xs transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "border-primary bg-accent text-foreground" : "text-muted-foreground hover:text-foreground"} ${known.has(t.slug) ? "" : "line-through"}`}
              >
                {t.name}
              </button>
            );
          })}
        </div>
      </Field>
      <Field label="年月">
        <MonthInput value={w.month || null} onChange={(m) => props.set({ month: m ?? "" })} />
      </Field>
      <Field label="封面">
        <Cover work={w} set={(cover) => props.set({ cover })} addImages={props.addImages} onError={props.onError} onPickAsset={props.onPickAsset} />
      </Field>
      <Field label="画廊">
        <button type="button" onClick={props.onGallery} className="flex items-center justify-between rounded-control border px-3 py-1.5 text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring">
          <span>{w.gallery.length ? `${w.gallery.length} 张` : "没有"}</span>
          <span className="text-xs text-muted-foreground">编辑…</span>
        </button>
      </Field>
      <Field label="slug">
        <div className="flex items-center justify-between gap-2">
          <span className="selectable min-w-0 truncate font-mono text-xs">{w.slug}</span>
          <button type="button" onClick={props.onRename} className="shrink-0 rounded px-1 text-xs text-muted-foreground hover:text-foreground">
            改名…
          </button>
        </div>
      </Field>
    </div>
  );
}

// ---------- the middle ----------

function VersionPane(props: { lang: Lang; v: Version; set: (patch: Partial<Version>) => void; editor: (el: MdEditorHandle | null) => void; onFocus: () => void; addImages: (take: { paths?: string[]; files?: File[] }) => Promise<string[]>; onError: (e: unknown) => void; draft?: ReactNode; locked?: boolean; note?: string | null }) {
  const { v, lang } = props;
  return (
    <div className="space-y-3" onFocusCapture={props.onFocus}>
      <input
        aria-label={`标题（${LABEL[lang]}）`}
        value={v.title}
        readOnly={props.locked}
        onChange={(e) => props.set({ title: e.target.value })}
        spellCheck={false}
        className="w-full bg-transparent text-[20px] font-bold outline-none placeholder:text-muted-foreground"
      />
      <Field label="摘要">
        <Polish module="site" value={v.summary} onChange={(summary) => props.set({ summary })}>
          <TextArea rows={2} value={v.summary} readOnly={props.locked} onChange={(e) => props.set({ summary: e.target.value })} className="text-[13px]" />
        </Polish>
      </Field>
      <div className="border-t pt-2">
        <BodyField lang={lang} label="正文" body={v.body} setBody={(body) => props.set({ body })} editor={props.editor} addImages={props.addImages} onError={props.onError} draft={props.draft} locked={props.locked} note={props.note} />
      </div>
    </div>
  );
}

function MissingVersion(props: { lang: Lang; zh: Version | null; setZh: (patch: Partial<Version>) => void; onCreate: () => void; busy: boolean; onError: (e: unknown) => void }) {
  const assistOn = useAssistOn();
  const filled = useFilled<"title" | "summary">();
  const [filling, setFilling] = useState(false);
  const zh = props.zh;
  const fill = async () => {
    if (!zh) return;
    setFilling(true);
    try {
      const r = await siteAssist.english(zh.title, zh.summary, zh.body);
      props.setZh({ englishTitle: r.title ?? zh.englishTitle, englishSummary: r.summary ?? zh.englishSummary });
      filled.mark([...(r.title ? ["title" as const] : []), ...(r.summary ? ["summary" as const] : [])]);
    } catch (e) {
      props.onError(e);
    } finally {
      setFilling(false);
    }
  };
  return (
    <div className="space-y-4 rounded-card border bg-card p-4">
      <p className="text-sm font-bold">{props.lang === "en" ? "还没有英文版" : "还没有中文版"}</p>
      {props.lang === "en" && zh && (
        <>
          <Field
            label="英文标题"
            action={
              assistOn && (
                <AssistAction disabled={filling} onClick={() => void fill()}>
                  {filling ? "正在补全…" : "补全"}
                </AssistAction>
              )
            }
          >
            <TextInput
              value={zh.englishTitle}
              onChange={(e) => {
                filled.clear("title");
                props.setZh({ englishTitle: e.target.value });
              }}
              className={filled.cls("title")}
            />
          </Field>
          <Field label="英文摘要">
            <Polish module="site" value={zh.englishSummary} onChange={(t) => props.setZh({ englishSummary: t })}>
              <TextArea
                rows={3}
                value={zh.englishSummary}
                onChange={(e) => {
                  filled.clear("summary");
                  props.setZh({ englishSummary: e.target.value });
                }}
                className={`text-[13px] ${filled.cls("summary")}`}
              />
            </Polish>
          </Field>
        </>
      )}
      <Button variant="primary" disabled={props.busy} onClick={props.onCreate}>
        {props.lang === "en" ? "新建英文版" : "新建中文版"}
      </Button>
    </div>
  );
}

// ---------- the editor ----------

export function WorkEditor(props: { slug: string; taxonomy: Taxonomy; view: View; setView: (v: View) => void; setDirty: (d: boolean) => void; onChanged: (slug?: string | null) => void; onError: (e: unknown) => void }) {
  const [confirm, confirmDialog] = useConfirm();
  const { data: loaded, refetch } = useQuery({ queryKey: [...SITE_KEY, "work", props.slug], queryFn: () => workApi.load(props.slug), gcTime: 0, staleTime: 0 });
  const [saved, setSaved] = useState<Work | null>(null);
  const [draft, setDraft] = useState<Work | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [dialog, setDialog] = useState<"gallery" | "rename" | "asset-cover" | "asset-body" | null>(null);
  const editors = useRef<Record<Lang, MdEditorHandle | null>>({ zh: null, en: null });
  const lastLang = useRef<Lang>("zh");
  useEffect(() => {
    if (loaded) {
      setSaved(loaded);
      setDraft(loaded);
    }
  }, [loaded]);
  const dirty = !!draft && !!saved && JSON.stringify(draft) !== JSON.stringify(saved);
  useEffect(() => props.setDirty(dirty), [dirty]); // eslint-disable-line react-hooks/exhaustive-deps

  // 按中文起草: Rust writes the English in memory; it comes into the English version as it is written
  const english = useEnglishDraft(
    props.slug,
    (m) => setDraft((d) => (d?.en ? { ...d, en: { ...d.en, title: m.title.trim() || d.en.title, summary: m.summary || d.en.summary, body: m.body ? stored(shown(m.body)) : d.en.body } } : d)),
    props.onError,
    !!draft,
  );
  const latest = useRef(draft);
  latest.current = draft;

  const set = (patch: Partial<Work>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const setVersion = (lang: Lang) => (patch: Partial<Version>) => setDraft((d) => (d && d[lang] ? { ...d, [lang]: { ...d[lang]!, ...patch } } : d));
  const addImages = addTo(`wander:${props.slug}`);

  /** Writes the draft; the work as now on disk (also when nothing changed), null when it failed. */
  const save = async (): Promise<Work | null> => {
    const sent = latest.current;
    if (!sent || !saved) return null;
    if (JSON.stringify(sent) === JSON.stringify(saved)) return saved;
    setBusy("save");
    try {
      const w = await workApi.save(sent);
      setSaved(w);
      setDraft((d) => (d === sent ? w : d ? { ...d, prints: w.prints } : w));
      props.onChanged();
      return w;
    } catch (e) {
      props.onError(e);
      return null;
    } finally {
      setBusy(null);
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && !e.shiftKey && e.key.toLowerCase() === "s" && !document.querySelector("dialog[open]")) {
        e.preventDefault();
        void save();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (!draft) return <div className="flex-1" />;
  const w = draft;
  const langs: Lang[] = props.view === "both" ? ["zh", "en"] : [props.view];
  const previewLang: Lang = props.view === "both" ? lastLang.current : props.view;

  const preview = async () => {
    if (!(await save())) return;
    const lang = w[previewLang] ? previewLang : w.zh ? "zh" : "en";
    setBusy("preview");
    try {
      await workApi.preview(pagePath(w.slug, lang));
    } catch (e) {
      props.onError(e);
    } finally {
      setBusy(null);
    }
  };
  const addVersion = async (lang: Lang) => {
    const now = await save();
    if (!now) return;
    setBusy("version");
    try {
      const nw = await workApi.addVersion(w.slug, lang, now.prints);
      setSaved(nw);
      setDraft(nw);
      props.onChanged();
    } catch (e) {
      props.onError(e);
    } finally {
      setBusy(null);
    }
  };
  const reload = async () => {
    if (dirty && !(await confirm({ title: "放弃没有保存的修改？", confirmLabel: "放弃修改", danger: true }))) return;
    const r = await refetch();
    if (r.data) {
      setSaved(r.data);
      setDraft(r.data);
    }
  };
  const remove = async () => {
    if (!(await confirm({ title: `删除「${w.zh?.title || w.en?.title || w.slug}」？`, message: [w.zh && "中文版", w.en && "英文版"].filter(Boolean).join("和") + "的文件一起删掉，图片留着。", confirmLabel: "删除", danger: true }))) return;
    try {
      await workApi.remove(w.slug, saved!.prints);
      props.setDirty(false);
      props.onChanged(null);
    } catch (e) {
      props.onError(e);
    }
  };
  const rename = async (to: string) => {
    const now = await save();
    if (!now) throw new Error("没能先保存");
    const nw = await workApi.rename(w.slug, to, now.prints);
    setDialog(null);
    props.setDirty(false);
    props.onChanged(nw.slug);
  };
  const startDraft = async () => {
    const { zh, en } = w;
    if (!zh || !en) return;
    if ((en.body.trim() || en.summary.trim()) && !(await confirm({ title: "用按中文起草的英文替换现在的英文版？", message: "标题、摘要和正文都换成新起草的，保存了才写进文件。", confirmLabel: "起草" }))) return;
    await english.start(zh);
  };
  const drafting = english.drafting;

  const insertBody = (lines: string[]) => {
    const lang = w[lastLang.current] && langs.includes(lastLang.current) ? lastLang.current : langs.find((l) => w[l]);
    if (lang) editors.current[lang]?.insertLines(lines);
  };

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center justify-between gap-3 border-b px-6 py-2">
        <Segmented ariaLabel="语言" value={props.view} onChange={props.setView} options={[{ value: "zh", label: "中文" }, { value: "en", label: "English" }, { value: "both", label: "并排" }]} />
        <div className="flex items-center gap-1.5">
          <Dropdown
            ariaLabel="插入图片"
            plain
            value={null as string | null}
            onChange={(v) => {
              if (v === "file")
                void pickImages(true).then((paths) => {
                  if (paths.length) addImages({ paths }).then((urls) => insertBody(urls.map((u) => `![](${u})`)), props.onError);
                });
              else setDialog("asset-body");
            }}
            options={[
              { value: "file", label: "选择图片…" },
              { value: "asset", label: "从资源库…" },
            ]}
            renderTrigger={() => (
              <>
                <ImagePlus size={14} strokeWidth={1.5} /> 插入图片
              </>
            )}
            triggerClassName={GHOST}
          />
          <Button disabled={!!busy} onClick={() => void preview()}>
            {busy === "preview" ? "正在打开…" : "预览"}
          </Button>
          <Button variant="primary" disabled={!dirty || !!busy} onClick={() => void save()}>
            {busy === "save" ? "正在保存…" : "保存"}
          </Button>
          <Dropdown
            ariaLabel="更多"
            plain
            align="end"
            value={null as string | null}
            onChange={(v) => (v === "reload" ? void reload() : v === "rename" ? setDialog("rename") : void remove())}
            options={[
              { value: "reload", label: "重新读取" },
              { value: "rename", label: "改 slug…" },
              { value: "delete", label: <span className="text-destructive">删除作品…</span> },
            ]}
            renderTrigger={() => <MoreHorizontal size={16} strokeWidth={1.5} />}
            triggerClassName={GHOST}
          />
        </div>
      </div>
      <div className="flex min-h-0 flex-1">
        <div className="-m-1 min-w-0 flex-1 overflow-y-auto p-1">
          <div className={`mx-auto grid gap-8 px-6 py-5 ${langs.length === 2 ? "max-w-[1200px] grid-cols-2" : "max-w-[760px]"}`}>
            {langs.map((l) =>
              w[l] ? (
                <VersionPane
                  key={l}
                  lang={l}
                  v={w[l]!}
                  set={setVersion(l)}
                  editor={(el) => (editors.current[l] = el)}
                  onFocus={() => (lastLang.current = l)}
                  addImages={addImages}
                  onError={props.onError}
                  locked={l === "en" && drafting}
                  note={l === "en" && drafting ? english.mine?.note : null}
                  draft={l === "en" && w.zh ? <DraftAction job={english.job} mine={english.mine} onStart={() => void startDraft()} onStop={english.stop} /> : undefined}
                />
              ) : (
                <MissingVersion key={l} lang={l} zh={w.zh} setZh={setVersion("zh")} onCreate={() => void addVersion(l)} busy={!!busy} onError={props.onError} />
              ),
            )}
          </div>
        </div>
        <aside className="-m-1 w-64 shrink-0 overflow-y-auto border-l p-1">
          <div className="px-4 py-5">
            <SharedFields work={w} tax={props.taxonomy} set={set} addImages={addImages} onError={props.onError} onGallery={() => setDialog("gallery")} onRename={() => setDialog("rename")} onPickAsset={() => setDialog("asset-cover")} />
          </div>
        </aside>
      </div>
      {dialog === "gallery" && (
        <GalleryDialog
          work={w}
          onClose={() => setDialog(null)}
          onDone={(gallery) => {
            set({ gallery });
            setDialog(null);
          }}
          addImages={addImages}
          onError={props.onError}
        />
      )}
      {dialog === "rename" && <RenameDialog slug={w.slug} onClose={() => setDialog(null)} onRename={rename} />}
      {(dialog === "asset-cover" || dialog === "asset-body") && (
        <AssetPicker
          onClose={() => setDialog(null)}
          onPick={(a) => {
            if (dialog === "asset-cover") set({ cover: a.path });
            else insertBody([`![${a.alt}](${a.url})`]);
            setDialog(null);
          }}
        />
      )}
      {confirmDialog}
    </div>
  );
}
