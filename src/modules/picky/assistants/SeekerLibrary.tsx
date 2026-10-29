// Seeker's records by page (requirements「Seeker」; kinds in Rust catalog.rs): 素材 (基本信息, the materials by
// category), 档案 (经历档案, 已确认事实 by group), 规则 (rules and 有效个人约定 by task, 个人范例), 网申. A
// material, 档案, rule, 范例 or 网申 answer is a card that opens; a fact or 约定 is a row. Each edit goes
// through RecordEditor (its kind's fields → 查看差异 → 确认保存, one undo step). 素材 and 规则 show the page's
// language (规则 also the ones for both); 档案 and 网申 are both languages'. Cards drag into the user's order
// within their group; 「选择」 ticks several to 启用 / 停用 / 删除 at once (select.tsx; one undo step each).
import { PickField } from "@/shared/PickField";
import { useState, type ReactNode } from "react";
import { ChevronRight, Copy, History, Pencil, Plus, Sparkles, Trash2 } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { InlineMd } from "@/shared/InlineMd";
import { Modal } from "@/shared/Modal";
import { Button, EmptyState, Field, IconButton, SearchInput, Switch, TextArea, TextInput } from "@/shared/ui";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { MarkdownView } from "../MarkdownView";
import { PointEditor } from "./SeekerPoint";
import { SeekerMaterialFiles } from "./SeekerFiles";
import { SeekerDiff, SeekerSort, recordText, when } from "./SeekerParts";
import { SUMMARY } from "./SeekerParts";
import { RATER_USES } from "./raterApi";
import { PickRow, SelectBar, SelectButton, SortGroup, picked, useSelect, type Select } from "./select";
import { GroupHead, PromptsButton, fedTo, useGroupActions, usePrompts } from "./SeekerGroups";
import { groupName, recordIn, seekerApi, seekerBatch, type SeekerCatalog, type SeekerImport, type SeekerLang, type SeekerPoint, type SeekerRecord, type SeekerWorkspace } from "./seekerApi";

export type SeekerWrite = (action: () => Promise<SeekerWorkspace>) => Promise<boolean>;
/** A rule's or 约定's category: which model task reads it (Rust model.rs `context`). */
export const USES = [{ value: "共用准则", label: "共用准则" }, { value: "素材润色", label: "素材润色" }, { value: "整体审查", label: "选材和审查" }, { value: "写作规则", label: "所有任务" }];
type Use = { value: string; label: string };
const useLabel = (c: string, uses: Use[] = USES) => uses.find((u) => u.value === c)?.label ?? c;
/** Rater's rules (题库维护, 面试) live with Seeker's records but show only on Rater's 规则 page. */
const isRater = (r: SeekerRecord) => RATER_USES.some((u) => u.value === r.category) && r.kind === "rule";
/** A rule's points, numbered (as models read them). */
function RulePoints(props: { points: SeekerPoint[] }) {
  return <ol className="list-decimal space-y-1 pl-5 text-sm leading-relaxed">{props.points.map((pt) => <li key={pt.id} className="break-words"><InlineMd text={pt.text} /></li>)}</ol>;
}
const MATERIALS = ["教育", "技能", "项目", "工作"];
/** English materials in the order of the resume (Rust workflow.rs `ORDER_EN`). */
const MATERIALS_EN = [SUMMARY, "技能", "教育", "项目", "工作", "证书"];
const materialsOf = (lang: string | undefined) => (lang === "en" ? MATERIALS_EN : MATERIALS);
/** A rule's, 约定's or 范例's language. */
const LANGS = [{ value: "cn", label: "中文" }, { value: "en", label: "English" }, { value: "", label: "中英共用" }];
const NAMES = ["姓名", "Name"];
const DOSSIER = "经历档案";
export const STACK = "技术栈";
/** What can be chosen for a resume: the 技术栈 line, then the points (catalog.rs `choosable`). */
export function choosable(r: SeekerRecord): SeekerPoint[] {
  const stack = r.stack?.trim();
  return [...(stack ? [{ id: STACK, text: r.lang === "en" ? `Tech Stack: ${stack}` : `技术栈：${stack}` }] : []), ...r.points.filter((p) => !(stack && p.id === STACK))];
}
const roleLabel = (category: string) => (category === "教育" ? "专业" : category === "工作" ? "职位" : "角色");
const blank = (kind: string, category: string, lang: string): SeekerRecord => ({ id: crypto.randomUUID(), kind, category, title: "", text: "", time: "", role: "", location: "", points: [], fields: [], stack: "", material: null, materialEn: null, lang, source: null, revision: 0, enabled: true });
const matches = (r: SeekerRecord, q: string) => !q || [r.title, r.text, r.role, r.stack, r.category, ...r.points.map((p) => p.text), ...r.fields.map((f) => `${f.label}${f.value}`)].join("\n").toLowerCase().includes(q.toLowerCase());

/** uses: the 用途 groups of the 规则 page (Rater passes its own; Seeker's leaves Rater's out). */
/** defaults: a group's built-in points while it has no rules (Rater's SKILL workflows). */
type PageProps = { lang: SeekerLang; catalog: SeekerCatalog; busy: boolean; write: SeekerWrite; aiBusy: boolean; onPolish: (id: string) => void; tools?: ReactNode; uses?: Use[]; defaults?: Record<string, string[]> };

/** Editing, history and deleting, shared by the pages. */
function useRecords(p: PageProps) {
  const [editing, setEditing] = useState<SeekerRecord | null>(null);
  const [past, setPast] = useState<string | null>(null);
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [confirm, confirmDialog] = useConfirm();
  const remove = async (r: SeekerRecord) => {
    if (await confirm({ title: `删除「${r.title}」？`, message: "已有简历不受影响；它的历史里还留着这一版。", danger: true, confirmLabel: "删除" })) void p.write(() => seekerApi.recordDelete(r.id));
  };
  const toggle = (id: string) => setOpen((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const dialogs = <>
    {editing && <RecordEditor initial={editing} catalog={p.catalog} busy={p.busy} write={p.write} uses={p.uses} close={() => setEditing(null)} />}
    {past && <HistoryDialog id={past} catalog={p.catalog} busy={p.busy} close={() => setPast(null)} edit={(r) => { setEditing(r); setPast(null); }} />}
    {confirmDialog}
  </>;
  // new facts and 网申 are both languages'; everything else the page's
  // a new rule starts with one empty point (or the given ones: 改成我的)
  return { add: (kind: string, category: string, text = "", title = "", points: string[] = kind === "rule" ? [""] : []) => setEditing({ ...blank(kind, category, ["fact", "application"].includes(kind) ? "" : p.lang), text, title, points: points.map((t) => ({ id: crypto.randomUUID(), text: t })) }), edit: (r: SeekerRecord) => setEditing(structuredClone(r)), history: setPast, remove, open, toggle, dialogs };
}
type Records = ReturnType<typeof useRecords>;

function Actions(props: { r: SeekerRecord; a: Records; p: PageProps; polish?: boolean; copy?: boolean }) {
  const toast = useToast();
  const { r, a, p } = props;
  return <div className="flex shrink-0 items-center opacity-0 transition-opacity duration-150 group-hover:opacity-100 focus-within:opacity-100">
    {props.copy && <IconButton label="复制" onClick={() => void navigator.clipboard.writeText(r.text).then(() => toast("已复制"), (e) => toast(errorText(e), "error"))}><Copy size={14} /></IconButton>}
    {props.polish && <IconButton label="润色" disabled={p.busy || p.aiBusy} onClick={() => p.onPolish(r.id)}><Sparkles size={14} /></IconButton>}
    <IconButton label="编辑" disabled={p.busy} onClick={() => a.edit(r)}><Pencil size={14} /></IconButton>
    <IconButton label="历史" onClick={() => a.history(r.id)}><History size={14} /></IconButton>
    <IconButton label="删除" danger disabled={p.busy} onClick={() => void a.remove(r)}><Trash2 size={14} /></IconButton>
  </div>;
}

function Frame(props: { query: string; setQuery: (q: string) => void; tools?: ReactNode; empty: boolean; children: ReactNode; dialogs: ReactNode; after?: ReactNode; bar?: ReactNode }) {
  return <div className="flex min-h-0 flex-1 flex-col gap-4 pt-4">
    <div className="flex items-center gap-2"><div className="w-64"><SearchInput aria-label="搜索资料" placeholder="搜索" value={props.query} onChange={(e) => props.setQuery(e.target.value)} /></div><span className="flex-1" />{props.tools}</div>
    {props.bar}
    <div className="-m-1 min-h-0 flex-1 space-y-6 overflow-y-auto p-1 pb-6">{props.empty ? <EmptyState title={props.query ? "没有找到" : "还没有资料"} /> : props.children}{props.after}</div>
    {props.dialogs}
  </div>;
}

function Group(props: { title: string; onAdd?: () => void; addLabel?: string; menu?: ReactNode; note?: string; children?: ReactNode }) {
  return <section className="space-y-2">
    <GroupHead title={props.title} add={props.onAdd && <IconButton label={props.addLabel ?? `新增${props.title}`} onClick={props.onAdd}><Plus size={14} /></IconButton>} menu={props.menu} note={props.note} />
    {props.children}
  </section>;
}

/** A card that opens: its head, then (open) its body. */
function Card(props: { r: SeekerRecord; a: Records; meta?: ReactNode; actions: ReactNode; children: ReactNode; sel?: Select; drag?: Record<string, unknown> }) {
  const open = props.a.open.has(props.r.id);
  const on = !!props.sel?.has(props.r.id);
  const selecting = !!props.sel?.on;
  return <article {...props.drag} className={`group rounded-card border ${on ? picked(true) : "bg-card"} ${props.r.enabled ? "" : "opacity-60"}`}>
    <div className="flex items-center gap-2 py-1.5 pr-2 pl-3">
      <button type="button" aria-expanded={selecting ? undefined : open} role={selecting ? "checkbox" : undefined} aria-checked={selecting ? on : undefined} onClick={selecting ? props.sel!.pick(props.r.id) : () => props.a.toggle(props.r.id)} className="flex min-w-0 flex-1 items-center gap-2 rounded-control py-1 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <ChevronRight size={14} className={`shrink-0 text-muted-foreground transition-transform duration-150 ${open ? "rotate-90" : ""}`} />
        <span className="truncate font-medium">{props.r.title || "未命名"}</span>
        {props.meta && <span className="min-w-0 truncate text-xs text-muted-foreground">{props.meta}</span>}
        {!props.r.enabled && <span className="shrink-0 text-xs text-muted-foreground">已停用</span>}
      </button>
      {!props.sel?.on && props.actions}
    </div>
    {open && <div className="border-t px-4 py-3 text-sm">{props.children}</div>}
  </article>;
}

/** Rows of name + text (已确认事实, 有效个人约定). */
function Rows(props: { rows: SeekerRecord[]; a: Records; p: PageProps; shared?: boolean; sel: Select }) {
  if (!props.rows.length) return null;
  return <SortGroup items={props.rows} disabled={props.p.busy || props.sel.on} onSort={(ids) => void props.p.write(() => seekerBatch.order(ids))} className="divide-y rounded-card border bg-card">{(r, drag) => <div {...drag} role={props.sel.on ? "checkbox" : undefined} aria-checked={props.sel.on ? props.sel.has(r.id) : undefined} onClick={props.sel.on ? props.sel.pick(r.id) : undefined} className={`group flex items-start gap-4 px-3 py-2 text-sm ${props.sel.has(r.id) ? "bg-accent text-accent-foreground" : "bg-card"} ${props.sel.on ? "cursor-pointer" : ""} ${r.enabled ? "" : "opacity-60"}`}>
    <span className="w-44 shrink-0 pt-1 font-medium">{r.title}{props.shared && r.lang === "" && <span className="ml-2 text-xs font-normal text-muted-foreground">中英共用</span>}</span>
    <p className="min-w-0 flex-1 pt-1 break-words whitespace-pre-wrap"><InlineMd text={r.text} /></p>
    {!props.sel.on && <Actions r={r} a={props.a} p={props.p} />}
  </div>}</SortGroup>;
}

/** Cards of one group in the user's order. */
function Cards(props: { rows: SeekerRecord[]; p: PageProps; sel: Select; children: (r: SeekerRecord, drag: Record<string, unknown>) => ReactNode }) {
  return <SortGroup items={props.rows} disabled={props.p.busy || props.sel.on} onSort={(ids) => void props.p.write(() => seekerBatch.order(ids))} className="space-y-2">{props.children}</SortGroup>;
}

/** 选择's actions for records: 启用, 停用, 删除 (asked first). */
function RecordBar(props: { sel: Select; p: PageProps }) {
  const [confirm, dialog] = useConfirm();
  const ids = [...props.sel.picked];
  const none = !ids.length || props.p.busy;
  const remove = async () => {
    if (await confirm({ title: `删除 ${ids.length} 条资料？`, message: "已有简历不受影响；它们的历史里还留着最后一版。", danger: true, confirmLabel: "删除" })) void props.p.write(() => seekerBatch.remove(ids)).then((ok) => ok && props.sel.exit());
  };
  return <SelectBar select={props.sel}>
    <Button disabled={none} onClick={() => void props.p.write(() => seekerBatch.enable(ids, true))}>启用</Button>
    <Button disabled={none} onClick={() => void props.p.write(() => seekerBatch.enable(ids, false))}>停用</Button>
    <Button variant="ghost-danger" disabled={none} onClick={() => void remove()}><Trash2 size={14} />删除</Button>
    {dialog}
  </SelectBar>;
}

const Markdown = ({ text }: { text: string }) => <MarkdownView source={text} className="text-sm" />;

export function SeekerMaterials(p: PageProps) {
  const a = useRecords(p);
  const [query, setQuery] = useState("");
  const records = p.catalog.records.filter((r) => (r.lang ?? "cn") === p.lang && matches(r, query));
  const profiles = records.filter((r) => r.kind === "profile");
  const materials = records.filter((r) => r.kind === "material");
  const order = materialsOf(p.lang);
  const categories = [...order, ...new Set(materials.map((r) => r.category).filter((c) => !order.includes(c)))];
  const dossierOf = (id: string) => p.catalog.records.find((d) => d.kind === "fact" && d.category === DOSSIER && (d.material === id || d.materialEn === id));
  const sel = useSelect([...profiles, ...categories.flatMap((c) => materials.filter((r) => r.category === c))].map((r) => r.id));
  const groups = useGroupActions(p.write, p.lang, p.busy);
  return <Frame query={query} setQuery={setQuery} tools={<>{p.tools}<SelectButton select={sel} /></>} bar={<RecordBar sel={sel} p={p} />} empty={!!query && !profiles.length && !materials.length} dialogs={<>{a.dialogs}{groups.dialogs}</>}>
    <Group title="基本信息" onAdd={profiles.length || query ? undefined : () => a.add("profile", "基本信息")}>
      {profiles.map((r) => <Card key={r.id} r={r} a={a} sel={sel} meta={r.fields.find((f) => NAMES.includes(f.label))?.value} actions={<Actions r={r} a={a} p={p} />}>
        <dl className="grid grid-cols-[6rem_1fr] gap-x-4 gap-y-1">{r.fields.map((f) => <div key={f.id} className="contents"><dt className="text-muted-foreground">{f.label}</dt><dd className="break-all">{f.value}</dd></div>)}</dl>
      </Card>)}
    </Group>
    {categories.map((c) => {
      const rows = materials.filter((r) => r.category === c);
      if (query && !rows.length) return null;
      const name = groupName(p.catalog, "material", p.lang, c);
      return <Group key={c} title={name} onAdd={() => a.add("material", c)} addLabel={`在「${name}」里新增`} menu={!query && groups.menu("material", c, name, p.catalog.records.filter((r) => r.kind === "material" && r.category === c && (r.lang ?? "cn") === p.lang).map((r) => r.id))}>
        <Cards rows={rows} p={p} sel={sel}>{(r, drag) => {
          const dossier = dossierOf(r.id);
          return <Card r={r} a={a} sel={sel} drag={drag} meta={[r.role, r.time, r.location].filter(Boolean).join(" · ")} actions={<Actions r={r} a={a} p={p} polish />}>
            <div className="space-y-2">
              {r.text && <p className={`whitespace-pre-wrap ${r.category === SUMMARY ? "" : "text-muted-foreground"}`}>{r.text}</p>}
              {r.stack && <p className="text-xs text-muted-foreground">技术栈：{r.stack}</p>}
              {r.points.length > 0 && <ul className="list-disc space-y-1 pl-5">{r.points.map((pt) => <li key={pt.id}><InlineMd text={pt.text} /></li>)}</ul>}
              {dossier && <p className="text-xs text-muted-foreground">经历档案：{dossier.title}</p>}
            </div>
          </Card>;
        }}</Cards>
      </Group>;
    })}
  </Frame>;
}

export function SeekerDossiers(p: PageProps) {
  const a = useRecords(p);
  const [query, setQuery] = useState("");
  const facts = p.catalog.records.filter((r) => r.kind === "fact" && matches(r, query));
  const dossiers = facts.filter((r) => r.category === DOSSIER);
  const groups = [...new Set(facts.filter((r) => r.category !== DOSSIER).map((r) => r.category))];
  const title = (id: string | null | undefined) => p.catalog.records.find((r) => r.id === id && r.kind === "material")?.title;
  const hung = (r: SeekerRecord) => [title(r.material), title(r.materialEn)].filter(Boolean).join(" · ");
  const sel = useSelect([...dossiers, ...groups.flatMap((g) => facts.filter((r) => r.category === g))].map((r) => r.id));
  return <Frame query={query} setQuery={setQuery} tools={<>{p.tools}<SelectButton select={sel} /><Button disabled={p.busy} onClick={() => a.add("fact", groups[0] ?? "个人事实")}><Plus size={14} />事实</Button></>} bar={<RecordBar sel={sel} p={p} />} empty={!facts.length} dialogs={a.dialogs}>
    {(dossiers.length > 0 || !query) && <Group title={DOSSIER} onAdd={() => a.add("fact", DOSSIER)}>
      <Cards rows={dossiers} p={p} sel={sel}>{(r, drag) => <Card r={r} a={a} sel={sel} drag={drag} meta={hung(r) ? `素材：${hung(r)}` : "没挂素材"} actions={<Actions r={r} a={a} p={p} />}><Markdown text={r.text} /></Card>}</Cards>
    </Group>}
    {groups.map((g) => <Group key={g} title={g} onAdd={() => a.add("fact", g)} addLabel={`在「${g}」里新增`}><Rows rows={facts.filter((r) => r.category === g)} a={a} p={p} sel={sel} /></Group>)}
  </Frame>;
}

export function SeekerRules(p: PageProps) {
  const a = useRecords(p);
  const [query, setQuery] = useState("");
  const own = p.uses ?? USES;
  const rater = !!p.uses;
  const records = p.catalog.records.filter((r) => ["rule", "example"].includes(r.kind) && recordIn(r, p.lang) && matches(r, query) && (rater ? own.some((u) => u.value === r.category) && r.kind !== "example" : !isRater(r)));
  const shared = (r: SeekerRecord) => (r.lang === "" && !rater ? "中英共用" : undefined);
  const uses = [...own.map((u) => u.value), ...new Set(records.filter((r) => r.kind !== "example").map((r) => r.category).filter((c) => !own.some((u) => u.value === c)))];
  const examples = rater ? [] : records.filter((r) => r.kind === "example");
  const shown = (u: string) => !((query || u === "写作规则") && !records.some((r) => r.kind !== "example" && r.category === u));
  const sel = useSelect([...uses.filter(shown).flatMap((u) => records.filter((r) => r.kind === "rule" && r.category === u)), ...examples].map((r) => r.id));
  const groups = useGroupActions(p.write, p.lang, p.busy);
  const prompts = usePrompts(p.lang);
  // a group's name: the user's for a built-in one, else the page's word for it
  const nameOf = (u: string) => { const custom = groupName(p.catalog, "rule", "", u); return custom !== u ? custom : useLabel(u, own); };
  return <Frame query={query} setQuery={setQuery} tools={<>{p.tools}<PromptsButton title={rater ? "Rater · 发给模型的说明" : "Seeker · 发给模型的说明"} list={rater ? prompts.rater : prompts.seeker} label={(g) => g === "个人范例" ? g : nameOf(g)} refetch={prompts.refetch} /><SelectButton select={sel} /></>} bar={<RecordBar sel={sel} p={p} />} empty={!!query && !records.length} dialogs={<>{a.dialogs}{groups.dialogs}</>}>
    {uses.map((u) => {
      const rules = records.filter((r) => r.kind === "rule" && r.category === u);
      if ((query || u === "写作规则") && !rules.length) return null;
      const name = nameOf(u);
      return <Group key={u} title={name} onAdd={() => a.add("rule", u)} addLabel={`在「${name}」里新增规则`} note={fedTo(u, prompts)} menu={!query && groups.menu("rule", u, name, p.catalog.records.filter((r) => r.kind === "rule" && r.category === u && recordIn(r, p.lang)).map((r) => r.id))}>
        <Cards rows={rules} p={p} sel={sel}>{(r, drag) => <Card r={r} a={a} sel={sel} drag={drag} meta={shared(r)} actions={<Actions r={r} a={a} p={p} />}><RulePoints points={r.points} /></Card>}</Cards>
        {!rules.length && p.defaults?.[u] && !query && <article className="rounded-card border border-dashed px-4 py-3 text-sm">
          <div className="mb-2 flex items-center gap-2"><span className="mr-auto text-xs text-muted-foreground">程序自带的工作流</span><Button variant="ghost" disabled={p.busy} onClick={() => a.add("rule", u, "", "工作流", p.defaults![u])}><Pencil size={13} />改成我的</Button></div>
          <RulePoints points={p.defaults[u].map((text, i) => ({ id: String(i), text }))} />
        </article>}
      </Group>;
    })}
    {!rater && (examples.length > 0 || !query) && <Group title="个人范例" note={fedTo("个人范例", prompts)} onAdd={() => a.add("example", "个人范例")}><Cards rows={examples} p={p} sel={sel}>{(r, drag) => <Card r={r} a={a} sel={sel} drag={drag} meta={shared(r)} actions={<Actions r={r} a={a} p={p} />}><Markdown text={r.text} /></Card>}</Cards></Group>}
  </Frame>;
}

export function SeekerApplications(p: PageProps) {
  const a = useRecords(p);
  const [query, setQuery] = useState("");
  const rows = p.catalog.records.filter((r) => r.kind === "application" && matches(r, query));
  const sel = useSelect(rows.map((r) => r.id));
  return <Frame query={query} setQuery={setQuery} tools={<>{p.tools}<SelectButton select={sel} /><Button disabled={p.busy} onClick={() => a.add("application", "网申")}><Plus size={14} />网申资料</Button></>} bar={<RecordBar sel={sel} p={p} />} empty={!rows.length} dialogs={a.dialogs} after={<SeekerMaterialFiles />}>
    <Cards rows={rows} p={p} sel={sel}>{(r, drag) => <Card r={r} a={a} sel={sel} drag={drag} meta={r.category !== "网申" ? r.category : undefined} actions={<Actions r={r} a={a} p={p} copy polish />}><p className="whitespace-pre-wrap">{r.text}</p></Card>}</Cards>
  </Frame>;
}

function HistoryDialog(props: { id: string; catalog: SeekerCatalog; busy: boolean; close: () => void; edit: (r: SeekerRecord) => void }) {
  const current = props.catalog.records.find((v) => v.id === props.id);
  const past = props.catalog.revisions.filter((r) => r.recordId === props.id);
  return <Modal size="xl" title="历史" error={null} onClose={props.close} onSubmit={(e) => { e.preventDefault(); props.close(); }} submitLabel="完成">
    <div className="-m-1 max-h-[58vh] space-y-5 overflow-y-auto p-1">
      {past.map((r) => <div key={r.id} className="space-y-2"><div className="flex items-center gap-2"><span className="mr-auto text-xs text-muted-foreground">{when(r.savedAt)}</span><Button variant="ghost" disabled={props.busy} onClick={() => props.edit({ ...r.body, revision: current?.revision ?? 0 })}>以这一版编辑</Button></div><SeekerDiff before={recordText(r.body)} after={recordText(current)} /></div>)}
      {!past.length && <p className="text-sm text-muted-foreground">还没有历史</p>}
    </div>
  </Modal>;
}

const KIND_TITLE: Record<string, string> = { material: "素材", profile: "基本信息", fact: "已确认事实", rule: "规则", example: "个人范例", application: "网申资料" };

/** One record's fields (by kind) → 查看差异 → 确认保存. */
/** uses: the 用途 to choose from (Rater's rules: its own, Chinese only, no language to pick). */
function RecordEditor(props: { initial: SeekerRecord; catalog: SeekerCatalog; busy: boolean; write: SeekerWrite; uses?: Use[]; close: () => void }) {
  const uses = props.uses ?? USES;
  const [v, setV] = useState(props.initial);
  const [review, setReview] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<SeekerRecord>) => setV((x) => ({ ...x, ...patch }));
  const dossier = v.kind === "fact" && v.category === DOSSIER;
  const name = dossier ? DOSSIER : KIND_TITLE[v.kind] ?? "资料";
  const input = (label: string, key: "title" | "role" | "time" | "location" | "stack" | "category") => <Field label={label}><TextInput value={v[key] ?? ""} onChange={(e) => set({ [key]: e.target.value })} /></Field>;
  const text = (label: string, rows: number) => <Field label={label}><TextArea rows={rows} value={v.text} onChange={(e) => set({ text: e.target.value })} /></Field>;
  const use = <Field label="用于"><Dropdown block ariaLabel="用于" value={v.category} options={uses.some((u) => u.value === v.category) ? uses : [...uses, { value: v.category, label: v.category }]} onChange={(category) => set({ category })} /></Field>;
  const language = props.uses ? null : <Field label="语言"><Dropdown block ariaLabel="语言" value={v.lang ?? "cn"} options={LANGS} onChange={(lang) => set({ lang })} /></Field>;
  const materials = props.catalog.records.filter((r) => r.kind === "material");
  const hang = (key: "material" | "materialEn", lang: string, label: string) => <Field label={label}><PickField<string | null> block ariaLabel={label} title={`档案挂到的${label}`} value={v[key] ?? null} placeholder="不挂" options={[{ value: null, label: "不挂" }, ...materials.filter((m) => (m.lang ?? "cn") === lang).map((m) => ({ value: m.id as string | null, label: m.title, group: m.category }))]} onChange={(id) => set({ [key]: id })} /></Field>;
  const body = (() => {
    switch (v.kind) {
      case "material":
        if (v.category === SUMMARY) return <>
          <div className="grid grid-cols-[1fr_8rem] gap-3">{input("名称", "title")}<Field label="分类"><Dropdown block ariaLabel="分类" value={v.category} options={[...new Set([...materialsOf(v.lang), v.category])].map((c) => ({ value: c, label: groupName(props.catalog, "material", v.lang ?? "cn", c) }))} onChange={(category) => set({ category })} /></Field></div>
          {text("正文", 6)}
        </>;
        return <>
          <div className="grid grid-cols-[1fr_8rem] gap-3">{input("名称", "title")}<Field label="分类"><Dropdown block ariaLabel="分类" value={v.category} options={[...new Set([...materialsOf(v.lang), v.category])].map((c) => ({ value: c, label: groupName(props.catalog, "material", v.lang ?? "cn", c) }))} onChange={(category) => set({ category })} /></Field></div>
          <div className="grid grid-cols-3 gap-3">{input(roleLabel(v.category), "role")}{input("时间", "time")}{input("地点", "location")}</div>
          <Field label={v.category === "项目" ? "概览" : "说明"}><TextInput value={v.text} onChange={(e) => set({ text: e.target.value })} /></Field>
          {(v.category === "项目" || v.category === "工作" || v.stack) && input("技术栈", "stack")}
          <Field label="要点" action={<button type="button" className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground" onClick={() => set({ points: [...v.points, { id: crypto.randomUUID(), text: "" }] })}><Plus size={12} />要点</button>}>
            <div className="space-y-1.5">{v.points.map((pt, i) => <div key={pt.id} className="flex items-start gap-1 rounded-control border bg-card py-1 pr-1 pl-2.5">
              <div className="min-w-0 flex-1 py-0.5 text-sm"><PointEditor label={`要点 ${i + 1}`} value={pt.text} disabled={props.busy} onChange={(t) => set({ points: v.points.map((x) => (x.id === pt.id ? { ...x, text: t } : x)) })} /></div>
              <IconButton label="删除这条要点" danger onClick={() => set({ points: v.points.filter((x) => x.id !== pt.id) })}><Trash2 size={13} /></IconButton>
            </div>)}{!v.points.length && <p className="text-sm text-muted-foreground">还没有要点</p>}</div>
          </Field>
        </>;
      case "profile":
        return <Field label="字段" action={<button type="button" className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground" onClick={() => set({ fields: [...v.fields, { id: crypto.randomUUID(), label: "", value: "", visible: true }] })}><Plus size={12} />字段</button>}>
          <div className="space-y-1.5">{v.fields.map((f) => <div key={f.id} className="grid grid-cols-[7rem_1fr_auto] items-center gap-2">
            <TextInput aria-label="字段名" value={f.label} onChange={(e) => set({ fields: v.fields.map((x) => (x.id === f.id ? { ...x, label: e.target.value } : x)) })} />
            <TextInput aria-label="字段值" value={f.value} onChange={(e) => set({ fields: v.fields.map((x) => (x.id === f.id ? { ...x, value: e.target.value } : x)) })} />
            <IconButton label="删除这个字段" danger onClick={() => set({ fields: v.fields.filter((x) => x.id !== f.id) })}><Trash2 size={13} /></IconButton>
          </div>)}</div>
        </Field>;
      case "fact":
        return dossier ? <>
          {input("名称", "title")}
          <div className="grid grid-cols-2 gap-3">{hang("material", "cn", "中文素材")}{hang("materialEn", "en", "英文素材")}</div>
          {text("正文", 18)}
        </> : <>
          <div className="grid grid-cols-[1fr_10rem] gap-3">{input("事项", "title")}{input("分组", "category")}</div>
          {text("口径", 4)}
        </>;
      case "rule":
        // each point one sentence; dragged into order (the order models read them in)
        return <><div className={`grid gap-3 ${language ? "grid-cols-[1fr_10rem_8rem]" : "grid-cols-[1fr_10rem]"}`}>{input("标题", "title")}{use}{language}</div>
          <Field label="要点" action={<button type="button" className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground" onClick={() => set({ points: [...v.points, { id: crypto.randomUUID(), text: "" }] })}><Plus size={12} />要点</button>}>
            <SeekerSort items={v.points} disabled={props.busy} onSort={(ids) => set({ points: ids.map((id) => v.points.find((x) => x.id === id)!) })}>{(pt, grip) => <div className="mb-1.5 flex items-start gap-1 rounded-control border bg-card py-1 pr-1 pl-1">
              <span className="flex w-6 shrink-0 items-center justify-end gap-0.5 pt-1 text-xs text-muted-foreground tabular-nums">{grip}{v.points.indexOf(pt) + 1}.</span>
              <div className="min-w-0 flex-1 py-0.5 text-sm"><PointEditor label={`要点 ${v.points.indexOf(pt) + 1}`} value={pt.text} disabled={props.busy} onChange={(t) => set({ points: v.points.map((x) => (x.id === pt.id ? { ...x, text: t } : x)) })} /></div>
              <IconButton label="删除这条要点" danger onClick={() => set({ points: v.points.filter((x) => x.id !== pt.id) })}><Trash2 size={13} /></IconButton>
            </div>}</SeekerSort>
            {!v.points.length && <p className="text-sm text-muted-foreground">还没有要点</p>}
          </Field></>;
      case "example":
        return <><div className="grid grid-cols-[1fr_8rem] gap-3">{input("名称", "title")}{language}</div>{text("正文", 10)}</>;
      default:
        return <>{input("名称", "title")}{text("正文", 10)}</>;
    }
  })();
  const toggle = v.kind === "material" ? "用于选材" : v.kind === "profile" || v.kind === "application" ? null : "发给模型";
  const ready = v.title.trim() && !(v.kind === "fact" && !v.category.trim()) && !(v.kind === "rule" && !v.points.some((x) => x.text.trim()));
  return <Modal size="xl" title={review ? "确认修改" : props.initial.revision ? `编辑${name}` : `新增${name}`} error={error} onClose={props.close} submitDisabled={props.busy || !ready} submitLabel={review ? "确认保存" : "查看差异"} footerLeft={review ? <Button disabled={props.busy} onClick={() => setReview(false)}>返回编辑</Button> : toggle ? <div className="flex items-center gap-2 text-sm"><Switch label={toggle} checked={v.enabled} onChange={(enabled) => set({ enabled })} /><span>{toggle}</span></div> : undefined} onSubmit={(e) => { e.preventDefault(); if (!review) { setReview(true); return; } void props.write(() => seekerApi.recordSave(v)).then((ok) => (ok ? props.close() : setError("没存上，请重试或重新打开这一条"))); }}>
    {review ? <SeekerDiff before={props.initial.revision ? recordText(props.initial) : ""} after={recordText(v)} /> : <fieldset disabled={props.busy} className="-m-1 max-h-[62vh] space-y-3 overflow-y-auto p-1">{body}</fieldset>}
  </Modal>;
}

const PAGE_OF: Record<string, string> = { material: "素材", profile: "素材", fact: "档案", rule: "规则", example: "规则", application: "网申" };
const pageOf = (r: SeekerRecord) => (isRater(r) ? "Rater 规则" : PAGE_OF[r.kind] ?? "");

/** transfer/ → records: per page, each new or changed item (ticked) and the ones the files don't have any more (ticked: remove). */
export function SeekerImportDialog(props: { preview: SeekerImport; busy: boolean; write: SeekerWrite; close: () => void }) {
  const items = props.preview.items;
  const gone = props.preview.gone ?? [];
  const [chosen, setChosen] = useState(() => new Set(items.filter((i) => !i.current).map((i) => i.incoming.id)));
  const [removing, setRemoving] = useState(() => new Set(gone.map((r) => r.id)));
  const [error, setError] = useState<string | null>(null);
  const flip = (set: Set<string>, id: string) => { const n = new Set(set); if (n.has(id)) n.delete(id); else n.add(id); return n; };
  const pages = ["素材", "档案", "规则", "网申", "Rater 规则"];
  const label = [chosen.size && `导入 ${chosen.size} 条`, removing.size && `删除 ${removing.size} 条`].filter(Boolean).join("，") || "没有要做的";
  const row = (id: string, on: boolean, flipIt: () => void, title: string, meta: string, diff?: ReactNode) => <div key={id} className="py-1.5">
    <PickRow on={on} label={title} onClick={flipIt} className="items-center gap-2 px-2 py-1"><span className="min-w-0 truncate">{title}</span><span className="shrink-0 text-xs text-muted-foreground">{meta}</span></PickRow>
    {diff && <details className="mt-1 pl-2"><summary className="cursor-pointer text-xs text-muted-foreground">差异</summary><div className="mt-1">{diff}</div></details>}
  </div>;
  return <Modal size="xl" title="从 transfer/ 导入" onClose={props.close} error={error} submitLabel={label} submitDisabled={props.busy || (!chosen.size && !removing.size)} onSubmit={(e) => { e.preventDefault(); void props.write(() => seekerApi.import(items.filter((i) => chosen.has(i.incoming.id)).map((i) => i.incoming), [...removing])).then((ok) => (ok ? props.close() : setError("没导入完；资料要是变了，请重新打开导入"))); }}>
    <fieldset disabled={props.busy} className="-m-1 max-h-[60vh] space-y-4 overflow-y-auto p-1">
      {pages.map((page) => {
        const here = items.filter((i) => pageOf(i.incoming) === page);
        if (!here.length) return null;
        return <section key={page}><div className="flex items-center gap-2"><h3 className="text-sm font-bold">{page}<span className="ml-2 font-normal text-muted-foreground">{here.length}</span></h3><button type="button" className="text-xs text-muted-foreground hover:text-foreground" onClick={() => setChosen((s) => new Set([...s, ...here.map((i) => i.incoming.id)]))}>全选</button></div><div className="divide-y">{here.map((i) => row(i.incoming.id, chosen.has(i.incoming.id), () => setChosen((s) => flip(s, i.incoming.id)), i.incoming.title, `${i.current ? "有变化" : "新的"} · ${i.incoming.lang === "en" ? "English · " : ""}${i.incoming.category}`, <SeekerDiff before={recordText(i.current)} after={recordText(i.incoming)} />))}</div></section>;
      })}
      {gone.length > 0 && <section><h3 className="text-sm font-bold">文件里没有了<span className="ml-2 font-normal text-muted-foreground">勾上的会删掉</span></h3><div className="divide-y">{gone.map((r) => row(r.id, removing.has(r.id), () => setRemoving((s) => flip(s, r.id)), r.title, `${pageOf(r)} · ${r.category}`))}</div></section>}
      {!items.length && !gone.length && <p className="text-sm text-muted-foreground">和 transfer/ 一样，没有要导入的</p>}
      {props.preview.skipped.length > 0 && <details><summary className="cursor-pointer text-xs text-muted-foreground">没导入的文件（{props.preview.skipped.length}）</summary><div className="mt-1 space-y-0.5">{props.preview.skipped.map((s, i) => <p key={i} className="text-xs text-muted-foreground">{s}</p>)}</div></details>}
    </fieldset>
  </Modal>;
}
