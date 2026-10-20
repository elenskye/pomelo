// 作品 (Wander): the list on the left (类别 · 语言 · 年月 filters, newest first, 无英文 marked) and the open
// work beside it (WorkEditor). 并排 hides the list. Unsaved edits are guarded when another work or page
// is opened.
import { useEffect, useMemo, useState, type FormEvent, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { useSetting } from "@/shared/settings";
import { EmptyState, Field, IconButton, Segmented, TextInput } from "@/shared/ui";
import { SITE_KEY, WORKS_KEY, useWorks, workApi, type Lang, type Taxonomy, type WorkRow } from "./api";
import { WorkEditor } from "./WorkEditor";
import type { View } from "./parts";

type LangFilter = "all" | "both" | "zh" | "en";
const LANG_OPTIONS: { value: LangFilter; label: string }[] = [
  { value: "all", label: "全部语言" },
  { value: "both", label: "中英都有" },
  { value: "zh", label: "只有中文" },
  { value: "en", label: "只有英文" },
];

export const categoryName = (tax: Taxonomy, slug: string) => tax.categories.find((c) => c.slug === slug)?.zh ?? slug;

const trigger = (sel: { label: ReactNode } | undefined) => <span className="truncate">{sel?.label}</span>;
const triggerClass = (on: boolean) =>
  `min-w-0 rounded-control px-1.5 py-0.5 text-xs outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring ${on ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground"}`;

function Filters(props: { tax: Taxonomy; months: string[]; cat: string; setCat: (v: string) => void; lang: LangFilter; setLang: (v: LangFilter) => void; month: string; setMonth: (v: string) => void }) {
  return (
    <div className="flex min-w-0 items-center gap-0.5">
      <Dropdown ariaLabel="类别" value={props.cat} onChange={props.setCat} renderTrigger={trigger} triggerClassName={triggerClass(props.cat !== "")} options={[{ value: "", label: "全部类别" }, ...props.tax.categories.map((c) => ({ value: c.slug, label: c.zh }))]} />
      <Dropdown ariaLabel="语言" value={props.lang} onChange={props.setLang} renderTrigger={trigger} triggerClassName={triggerClass(props.lang !== "all")} options={LANG_OPTIONS} />
      <Dropdown ariaLabel="年月" value={props.month} onChange={props.setMonth} renderTrigger={trigger} triggerClassName={triggerClass(props.month !== "")} menuMaxHeight={320} options={[{ value: "", label: "全部年月" }, ...props.months.map((m) => ({ value: m, label: m.replace("-", " / ") }))]} />
    </div>
  );
}

function Row(props: { w: WorkRow; tax: Taxonomy; selected: boolean; onOpen: () => void }) {
  const { w } = props;
  const bits = [w.month.replace("-", " / "), w.category ? categoryName(props.tax, w.category) : null].filter(Boolean).join(" · ");
  return (
    <button
      type="button"
      onClick={props.onOpen}
      className={`flex w-full flex-col gap-0.5 rounded-control px-3 py-2 text-left transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${props.selected ? "bg-accent" : "hover:bg-muted/70"}`}
    >
      <span className="truncate text-sm">{w.title || w.slug}</span>
      <span className="flex items-center gap-1.5 text-xs text-muted-foreground tabular-nums">
        <span className="truncate">{bits}</span>
        {!w.langs.includes("en") && <span className="shrink-0 rounded-full bg-muted px-1.5 text-[11px]">无英文</span>}
        {!w.langs.includes("zh") && <span className="shrink-0 rounded-full bg-muted px-1.5 text-[11px]">无中文</span>}
      </span>
    </button>
  );
}

function NewWorkDialog(props: { onClose: () => void; onCreated: (slug: string) => void }) {
  const [title, setTitle] = useState("");
  const [slug, setSlug] = useState("");
  const [lang, setLang] = useState<Lang>("zh");
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const w = await workApi.create(slug.trim(), lang, title);
      props.onCreated(w.slug);
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal title="新作品" onClose={props.onClose} onSubmit={(e) => void submit(e)} error={error} submitLabel="新建" submitDisabled={!title.trim() || !slug.trim()}>
      <Segmented ariaLabel="语言" value={lang} onChange={setLang} options={[{ value: "zh", label: "中文" }, { value: "en", label: "English" }]} />
      <Field label="标题">
        <TextInput autoFocus value={title} onChange={(e) => setTitle(e.target.value)} />
      </Field>
      <Field label="slug（网址里的名字）">
        <TextInput value={slug} onChange={(e) => setSlug(e.target.value.toLowerCase())} className="font-mono" />
      </Field>
    </Modal>
  );
}

export function Works(props: { guard: { dirty: boolean; confirmLeave: () => Promise<boolean>; setDirty: (d: boolean) => void } }) {
  const qc = useQueryClient();
  const toast = useToast();
  const { data } = useWorks(true);
  const [selected, setSelected] = useState<string | null>(null);
  const [view, setView] = useSetting<View>("ui.site_work_view", "zh", (v): v is View => v === "zh" || v === "en" || v === "both");
  const [cat, setCat] = useState("");
  const [lang, setLang] = useState<LangFilter>("all");
  const [month, setMonth] = useState("");
  const [creating, setCreating] = useState(false);
  const works = data?.works ?? [];
  const tax = data?.taxonomy ?? { categories: [], topics: [] };
  const months = useMemo(() => [...new Set(works.map((w) => w.month).filter(Boolean))].sort().reverse(), [works]);
  const shown = works.filter(
    (w) =>
      (!cat || w.category === cat) &&
      (!month || w.month === month) &&
      (lang === "all" || (lang === "both" ? w.langs.length === 2 : w.langs.length === 1 && w.langs[0] === lang)),
  );
  // open the newest one first; a deleted / renamed one falls back too
  useEffect(() => {
    if (data && (!selected || !works.some((w) => w.slug === selected))) setSelected(works[0]?.slug ?? null);
  }, [data]); // eslint-disable-line react-hooks/exhaustive-deps

  const open = async (slug: string | null) => {
    if (slug === selected) return;
    if (await props.guard.confirmLeave()) {
      props.guard.setDirty(false);
      setSelected(slug);
    }
  };
  const changed = (slug?: string | null) => {
    void qc.invalidateQueries({ queryKey: WORKS_KEY });
    void qc.invalidateQueries({ queryKey: [...SITE_KEY, "changes"] });
    if (slug !== undefined) setSelected(slug);
  };

  if (!data) return null;
  return (
    <div className="flex min-h-0 flex-1 border-t">
      {view !== "both" && (
        <aside className="flex w-60 shrink-0 flex-col border-r">
          <div className="flex items-center justify-between gap-1 px-3 py-2">
            <Filters tax={tax} months={months} cat={cat} setCat={setCat} lang={lang} setLang={setLang} month={month} setMonth={setMonth} />
            <IconButton label="新作品" onClick={() => void props.guard.confirmLeave().then((ok) => ok && setCreating(true))}>
              <Plus size={16} strokeWidth={1.5} />
            </IconButton>
          </div>
          <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
            <div className="space-y-0.5 px-2 pb-4">
              {shown.map((w) => (
                <Row key={w.slug} w={w} tax={tax} selected={w.slug === selected} onOpen={() => void open(w.slug)} />
              ))}
              {data.broken.map((b) => (
                <p key={b} className="px-3 py-1 text-xs text-destructive">
                  {b}
                </p>
              ))}
            </div>
          </div>
        </aside>
      )}
      {selected ? (
        <WorkEditor
          key={selected}
          slug={selected}
          taxonomy={tax}
          view={view}
          setView={setView}
          setDirty={props.guard.setDirty}
          onChanged={changed}
          onError={(e) => toast(errorText(e), "error")}
        />
      ) : (
        <EmptyState title="作品还空着" />
      )}
      {creating && (
        <NewWorkDialog
          onClose={() => setCreating(false)}
          onCreated={(slug) => {
            setCreating(false);
            props.guard.setDirty(false);
            changed(slug);
          }}
        />
      )}
    </div>
  );
}
