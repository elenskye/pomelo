// Seeker in the browser mock: fictional materials (林小柚), a simulated transfer/ preview, template,
// pagination (2 pages once the draft has over 24 points) and exports (no file is written).
import type { SeekerCatalog, SeekerDraft, SeekerEntry, SeekerExport, SeekerImport, SeekerRecord, SeekerSelection, SeekerSettings, SeekerTemplate, SeekerWorkspace, SourceChange } from "@/modules/picky/assistants/seekerApi";

const HEADER = ["工作邮箱", "联系电话", "个人网站", "工作地点"];
const rec = (r: Partial<SeekerRecord> & { id: string; title: string }): SeekerRecord => ({ kind: "material", category: "项目", text: "", time: "", role: "", location: "", points: [], fields: [], source: null, revision: 1, enabled: true, ...r });
const pts = (...texts: string[]) => texts.map((text) => ({ id: text.match(/^\*\*(.+?)\*\*/)?.[1] ?? (text.startsWith("技术栈") ? "技术栈" : text), text }));
const sample = (): SeekerCatalog => ({
  records: [
    rec({ id: "r-profile", kind: "profile", category: "基本信息", title: "基本信息", source: "cn-modules/1 - 基本信息.md#基本信息:1", fields: [["姓名", "林小柚"], ["工作地点", "上海"], ["工作邮箱", "work@example.com"], ["个人邮箱", "me@example.com"], ["联系电话", "+86 100 0000 0000"], ["GitHub", "github.com/example"], ["个人网站", "example.com"]].map(([label, value], i) => ({ id: `f${i + 1}`, label, value, visible: true })) }),
    rec({ id: "r-edu", category: "教育", title: "示例大学 · 信息学院", role: "数据科学 · 硕士", time: "2025.07 - 至今", points: pts("**主修课程**：机器学习、数据库") }),
    rec({ id: "r-skill", category: "技能", title: "技能证书", points: pts("**编程语言**：Python、SQL", "**语言能力**：英语六级") }),
    rec({ id: "r-notes", title: "柚子笔记", role: "独立开发", text: "读书笔记整理工具", time: "2026.03 - 至今", points: pts("技术栈：Rust、SQLite", "**数据整理**：把零散笔记合成卷", "**检索改进**：按意思搜索") }),
    rec({ id: "r-orange", title: "橙子识别", role: "课程项目", text: "水果分类模型", time: "2025.10 - 2025.12", location: "上海", points: pts("**模型迭代**：两轮调参，准确率 0.91", "**类别均衡**：重采样处理样本失衡") }),
    rec({ id: "r-work", category: "工作", title: "示例证券 · 研究所", role: "研究部实习生", time: "2024.01 - 2024.02", location: "杭州", points: pts("**行业研究**：整理研报", "**数据分析**：写脚本汇总收益") }),
    rec({ id: "r-rule", kind: "rule", category: "素材润色", title: "中文素材润色规则", text: "- 不编造事实\n- 小标题四个字" }),
    rec({ id: "r-apply", kind: "application", category: "网申", title: "为什么选择我们", text: "示例答案。" }),
  ],
  revisions: [],
  versions: [],
  settings: { selectionModel: null, reviewModel: null, thinking: false, cloudAllowed: false, developerRole: false },
});
const TEMPLATE_PARAGRAPHS = ["[姓名]", "[联系电话] · [电子邮箱] · [所在城市]", "教育背景", "[学校名称] · [专业名称]\t20XX.XX - 20XX.XX", "[学历或学位] · [学院名称]；[主修课程]", "编程开发：[技术一]、[技术二]", "20XX.XX - 20XX.XX\t[项目名称]\t[本人角色]\t[地点]", null];

type Job = { id: number; company: string; position: string | null; location: string | null; link: string | null; note: string | null; createdAt: string };
/** `job`: 投石问路's jobs (mockIpc sets it). */
export const seekerMock: { workspace: SeekerWorkspace; job: (id: number) => Job | undefined } = { workspace: { folder: "", draft: null, updatedAt: null, baseline: null, catalog: sample(), output: { template: null, exports: [], links: [] } }, job: () => undefined };
const catalog = () => seekerMock.workspace.catalog;
const isStack = (text: string) => /^(技术栈|tech stack)/i.test(text);
const view = () => {
  const c = catalog();
  const w = seekerMock.workspace;
  const warnings: string[] = [];
  for (const category of ["项目", "工作"]) {
    const count = w.draft?.sections.filter((s) => s.category === category).reduce((n, s) => n + s.entries.length, 0) ?? 0;
    if (count > 3) warnings.push(`${category}经历 ${count} 段，建议 3 段以内`);
  }
  for (const e of w.draft?.sections.flatMap((s) => s.entries) ?? []) if (e.source && !c.records.some((r) => r.id === e.source!.recordId)) warnings.push(`「${e.name}」的素材已删除`);
  const choices = c.records.filter((r) => r.kind === "material" && r.enabled).map((r) => ({ recordId: r.id, pointIds: r.points.filter((p) => c.settings.developerRole || !isStack(p.text)).map((p) => p.id) }));
  return structuredClone({ ...w, warnings, choices });
};
const save = (draft: SeekerDraft) => {
  const ids = [...draft.header.map((h) => h.id), ...draft.sections.flatMap((s) => [s.id, ...s.entries.flatMap((e) => [e.id, ...e.points.map((p) => p.id)])])];
  if (ids.some((id) => !id) || new Set(ids).size !== ids.length) throw new Error("简历中有空白或重复的块编号");
  if (JSON.stringify(seekerMock.workspace.draft) !== JSON.stringify(draft)) { seekerMock.workspace.draft = structuredClone(draft); seekerMock.workspace.updatedAt = new Date().toISOString(); }
  return view();
};
const entry = (r: SeekerRecord, pointIds: string[], id: string = crypto.randomUUID()): SeekerEntry => ({ id, time: r.time, name: r.title, role: r.role, location: r.location, note: r.text, points: r.points.filter((p) => pointIds.includes(p.id)).map((p) => ({ id: `${id}:${p.id}`, text: p.text })), source: { recordId: r.id, revision: r.revision, pointIds } });
const title = (category: string) => category === "教育" ? "教育背景" : category === "技能" ? "专业技能" : `${category}经历`;
const initial = (): SeekerDraft => {
  const profile = catalog().records.find((r) => r.kind === "profile" && r.enabled);
  const value = (label: string) => profile?.fields.find((f) => f.label === label)?.value ?? "";
  const header = HEADER.map((label, i) => ({ id: `header-${i}`, label, value: value(label), visible: true }));
  for (const f of profile?.fields ?? []) if (f.label !== "姓名" && !HEADER.includes(f.label)) header.push({ id: `header-${header.length}`, label: f.label, value: f.value, visible: false });
  return { name: value("姓名"), header, sections: [{ id: crypto.randomUUID(), category: "教育", title: "教育背景", entries: catalog().records.filter((r) => r.kind === "material" && r.category === "教育" && r.enabled).map((r) => entry(r, r.points.map((p) => p.id))) }], jd: "" };
};
const put = (input: SeekerRecord) => {
  if (!input.id || !input.title.trim()) throw new Error("请填写名称");
  const c = catalog();
  const prior = c.records.find((r) => r.id === input.id);
  if ((prior?.revision ?? 0) !== input.revision) throw new Error("这条资料刚刚改过，请重新打开");
  if (JSON.stringify(prior) === JSON.stringify(input)) return;
  if (input.source && c.records.some((r) => r.id !== input.id && r.source === input.source)) throw new Error("这个来源已经导入过了");
  if (prior) c.revisions.unshift({ id: Math.max(0, ...c.revisions.map((r) => r.id)) + 1, recordId: prior.id, body: structuredClone(prior), savedAt: new Date().toISOString() });
  const record = { ...structuredClone(input), revision: (prior?.revision ?? 0) + 1 };
  const index = c.records.findIndex((r) => r.id === record.id);
  if (index < 0) c.records.push(record); else c.records[index] = record;
};
let prepared: { ticket: string; draft: SeekerDraft; template: SeekerTemplate; pages: number } | null = null;

type Args = Record<string, unknown>;
export const seekerHandlers: Record<string, (a: Args) => unknown> = {
  picky_seeker: view,
  picky_seeker_folder: (a) => { seekerMock.workspace.folder = String(a.folder).trim(); return view(); },
  picky_seeker_settings: (a) => { catalog().settings = structuredClone(a.settings as SeekerSettings); return view(); },
  picky_seeker_template_read: () => structuredClone(TEMPLATE_PARAGRAPHS),
  picky_seeker_template: (a) => { seekerMock.workspace.output.template = structuredClone(a.template as SeekerTemplate | null); return view(); },
  picky_seeker_output_preview: () => {
    const w = seekerMock.workspace;
    if (!w.draft) throw new Error("还没有简历草稿");
    if (!w.output.template) throw new Error("请先在「模板与规则」里选好 Word 模板");
    const points = w.draft.sections.reduce((n, s) => n + s.entries.reduce((m, e) => m + e.points.length + 1, 0), 0);
    prepared = { ticket: crypto.randomUUID().replace(/-/g, ""), draft: structuredClone(w.draft), template: structuredClone(w.output.template), pages: points > 24 ? 2 : 1 };
    return { ticket: prepared.ticket, pages: prepared.pages, pdfPath: "/tmp/pomelo-seeker-mock/resume.pdf", defaultFolder: w.folder ? `${w.folder}/cv/cn` : "", renderer: "LibreOffice" };
  },
  picky_seeker_output_discard: () => { prepared = null; },
  picky_seeker_output_save: (a) => {
    const w = seekerMock.workspace;
    const done = w.output.exports.find((e) => e.id === a.ticket);
    if (done) return { workspace: view(), export: done };
    if (!prepared || prepared.ticket !== a.ticket) throw new Error("预览已失效，请重新分页");
    if (prepared.pages !== 1) throw new Error(`现在是 ${prepared.pages} 页，请精简后重新分页`);
    if (JSON.stringify(w.draft) !== JSON.stringify(prepared.draft)) throw new Error("草稿改过了，请重新分页");
    const filename = String(a.filename).trim();
    if (!/\.docx$/i.test(filename) || /[/\\:]/.test(filename)) throw new Error("文件名要以 .docx 结尾，不能有 / \\ :");
    const path = `${String(a.directory).trim()}/${filename}`;
    if (w.output.exports.some((e) => e.path === path)) throw new Error("已经有同名的文件了，请换个名字");
    const item: SeekerExport = { id: prepared.ticket, label: filename, draft: prepared.draft, path, pages: 1, renderer: "LibreOffice", template: prepared.template, createdAt: new Date().toISOString() };
    w.output.exports.unshift(item);
    prepared = null;
    if (a.jobId != null) seekerHandlers.picky_seeker_job_link({ exportId: item.id, jobId: a.jobId });
    return { workspace: view(), export: item };
  },
  picky_seeker_job_link: (a) => {
    const output = seekerMock.workspace.output;
    const item = output.exports.find((e) => e.id === a.exportId);
    const job = seekerMock.job(a.jobId as number);
    if (!item || !job) throw new Error("岗位或导出版本不存在");
    const previous = output.links.find((l) => l.exportId === item.id && l.job.id === job.id && l.job.createdAt === job.createdAt);
    for (const link of output.links) if (link.job.id === job.id && link.job.createdAt === job.createdAt) link.current = false;
    if (previous) previous.current = true;
    else output.links.unshift({ id: crypto.randomUUID(), exportId: item.id, job: { id: job.id, company: job.company, position: job.position, location: job.location, link: job.link, note: job.note, createdAt: job.createdAt }, jd: item.draft.jd, createdAt: new Date().toISOString(), current: true });
    return view();
  },
  picky_seeker_export_restore: (a) => { const item = seekerMock.workspace.output.exports.find((e) => e.id === a.exportId); if (!item) throw new Error("这次导出已经不在了"); save(item.draft); seekerMock.workspace.baseline = structuredClone(item.draft); return view(); },
  picky_seeker_new: () => { const draft = initial(); save(draft); seekerMock.workspace.baseline = structuredClone(draft); return view(); },
  picky_seeker_save: (a) => save(a.draft as SeekerDraft),
  picky_seeker_delete_blocks: (a) => {
    const draft = structuredClone(seekerMock.workspace.draft);
    if (!draft) throw new Error("还没有简历草稿");
    const ids = new Set(a.ids as string[]);
    draft.header = draft.header.filter((h) => !ids.has(h.id));
    draft.sections = draft.sections.filter((s) => !ids.has(s.id));
    draft.sections.forEach((s) => { s.entries = s.entries.filter((e) => !ids.has(e.id)); s.entries.forEach((e) => { e.points = e.points.filter((p) => !ids.has(p.id)); }); });
    return save(draft);
  },
  picky_seeker_record_save: (a) => { put(a.record as SeekerRecord); return view(); },
  picky_seeker_record_delete: (a) => {
    const c = catalog(), prior = c.records.find((r) => r.id === a.id);
    if (!prior) throw new Error("这条资料已经删除了");
    c.revisions.unshift({ id: Math.max(0, ...c.revisions.map((r) => r.id)) + 1, recordId: prior.id, body: structuredClone(prior), savedAt: new Date().toISOString() });
    c.records = c.records.filter((r) => r.id !== a.id); return view();
  },
  picky_seeker_import_preview: (): SeekerImport => {
    if (!seekerMock.workspace.folder) throw new Error("请先选择 Seeker 文件夹");
    const records = catalog().records;
    const fresh = rec({ id: crypto.randomUUID(), title: "柚子笔记 2", role: "独立开发", text: "示例：新导入的项目", time: "2026.08 - 至今", points: pts("**功能实现**：示例要点"), source: "cn-modules/4 - 项目经历.md#柚子笔记 2:1", revision: 0 });
    const old = records.find((r) => r.id === "r-orange");
    const items: SeekerImport["items"] = [{ incoming: fresh, current: null }];
    if (old) items.push({ incoming: { ...structuredClone(old), source: old.source ?? "cn-modules/4 - 项目经历.md#橙子识别:1", points: pts("**模型迭代**：三轮调参，准确率 0.93", "**类别均衡**：重采样处理样本失衡") }, current: structuredClone(old) });
    return { items: items.filter((i) => !records.some((r) => r.source === i.incoming.source && r.id !== i.incoming.id)), skipped: ["TRANSFER.md：不是要导入的资料", "en-modules/1-SUMMARY.md：英文资料以后再导入", "rules/qbank.md：面试题库以后再导入"] };
  },
  picky_seeker_import_confirm: (a) => {
    const prior = structuredClone(seekerMock.workspace);
    try { (a.records as SeekerRecord[]).forEach(put); } catch (e) { seekerMock.workspace = prior; throw e; }
    return view();
  },
  picky_seeker_assemble: (a) => {
    const picks = a.selections as SeekerSelection[];
    const current = seekerMock.workspace.draft;
    const fresh = !!a.replace || !current;
    const draft = fresh ? initial() : structuredClone(current!);
    if (fresh) { draft.jd = current?.jd ?? ""; draft.sections.forEach((s) => { s.entries = s.entries.filter((e) => !picks.some((p) => p.recordId === e.source?.recordId)); }); }
    for (const pick of picks) {
      const r = catalog().records.find((r) => r.id === pick.recordId && r.kind === "material" && r.enabled);
      if (!r) throw new Error("所选素材已删除或停用，请重新选材");
      if (pick.pointIds.some((id) => !r.points.some((p) => p.id === id))) throw new Error("素材的要点刚刚改过，请重新选择");
      if (draft.sections.some((s) => s.entries.some((e) => e.source?.recordId === r.id))) continue;
      const s = draft.sections.find((s) => s.category === r.category);
      if (s) s.entries.push(entry(r, pick.pointIds)); else draft.sections.push({ id: crypto.randomUUID(), category: r.category, title: title(r.category), entries: [entry(r, pick.pointIds)] });
    }
    save(draft); if (fresh) seekerMock.workspace.baseline = structuredClone(draft); return view();
  },
  picky_seeker_sources: (): SourceChange[] => (seekerMock.workspace.draft?.sections.flatMap((s) => s.entries) ?? []).flatMap((e) => {
    const r = catalog().records.find((r) => r.id === e.source?.recordId);
    if (!r || !e.source || r.revision === e.source.revision) return [];
    return [{ entryId: e.id, title: r.title, before: structuredClone(e), after: entry(r, e.source.pointIds.filter((id) => r.points.some((p) => p.id === id)), e.id) }];
  }),
  picky_seeker_source_apply: (a) => {
    const change = a.change as SourceChange, draft = structuredClone(seekerMock.workspace.draft);
    const section = draft?.sections.find((s) => s.entries.some((e) => e.id === change.entryId));
    const index = section?.entries.findIndex((e) => e.id === change.entryId) ?? -1;
    if (!draft || !section || index < 0 || JSON.stringify(section.entries[index]) !== JSON.stringify(change.before)) throw new Error("草稿刚刚改过，请重新看来源差异");
    section.entries[index] = structuredClone(change.after); return save(draft);
  },
  picky_seeker_confirm: (a) => {
    const body = seekerMock.workspace.draft; if (!body) throw new Error("还没有草稿");
    const c = catalog();
    c.versions.unshift({ id: Math.max(0, ...c.versions.map((v) => v.id)) + 1, label: String(a.label).trim() || "确认版本", body: structuredClone(body), createdAt: new Date().toISOString() });
    seekerMock.workspace.baseline = structuredClone(body); return view();
  },
  picky_seeker_restore: (a) => { const v = catalog().versions.find((v) => v.id === a.versionId); if (!v) throw new Error("找不到这个版本"); save(v.body); seekerMock.workspace.baseline = structuredClone(v.body); return view(); },
  picky_seeker_reorder: (a) => {
    const draft = structuredClone(seekerMock.workspace.draft); if (!draft) throw new Error("还没有草稿");
    const ids = a.ids as string[];
    const rows = a.kind === "header" ? draft.header : a.kind === "section" ? draft.sections : a.kind === "entry" ? draft.sections.find((s) => s.id === a.parent)?.entries : a.kind === "point" ? draft.sections.flatMap((s) => s.entries).find((e) => e.id === a.parent)?.points : undefined;
    if (!rows || ids.length !== rows.length || new Set(ids).size !== ids.length || rows.some((r) => !ids.includes(r.id))) throw new Error("内容刚刚变了，请重新排序");
    rows.sort((l, r) => ids.indexOf(l.id) - ids.indexOf(r.id)); return save(draft);
  },
  picky_seeker_apply: (a) => { if (JSON.stringify(seekerMock.workspace.draft) !== JSON.stringify(a.before)) throw new Error("草稿刚刚改过，请重新生成建议"); return save(a.after as SeekerDraft); },
  picky_seeker_suggest: (a) => {
    const c = catalog();
    const model = a.kind === "select" ? c.settings.selectionModel : c.settings.reviewModel;
    if (!model) throw new Error("请为这项工作选择模型");
    if (model.startsWith("cloud:") && !c.settings.cloudAllowed) throw new Error("Seeker 资料还没允许发给云端");
    const draft = a.kind === "review" || a.kind === "polish" ? structuredClone(seekerMock.workspace.draft) : null;
    if ((a.kind === "review" || a.kind === "polish") && !draft) throw new Error("还没有简历草稿");
    const record = a.kind === "material" ? structuredClone(c.records.find((r) => r.id === a.recordId) ?? null) : null;
    const base = { kind: a.kind as string, model: "模拟模型", note: "", advice: "", selections: [] as SeekerSelection[], beforeDraft: draft, afterDraft: null as SeekerDraft | null, beforeRecord: record, afterRecord: null as SeekerRecord | null };
    if (a.kind === "select") {
      if (!seekerMock.workspace.draft?.jd.trim()) throw new Error("请先填写目标 JD");
      return { ...base, advice: "（模拟）JD 偏数据分析：选两个项目和实习。", selections: c.records.filter((r) => r.kind === "material" && r.enabled && ["项目", "工作"].includes(r.category)).slice(0, 3).map((r) => ({ recordId: r.id, pointIds: r.points.filter((p) => !isStack(p.text)).map((p) => p.id) })) };
    }
    if (a.kind === "review") return { ...base, advice: "需处理\n- 「柚子笔记」第 2 条 → 缺少结果 → 补一个可核实的效果\n\n可选优化\n- 工作经历 → 动词重复 → 换一个更具体的动作" };
    if (draft) { const after = structuredClone(draft); const p = after.sections.flatMap((s) => s.entries).flatMap((e) => e.points)[0]; if (p) p.text = `${p.text}（模拟润色）`; return { ...base, advice: "（模拟）只改了第一条。", afterDraft: after }; }
    if (record) return { ...base, advice: "（模拟）概览更具体。", afterRecord: { ...record, text: `${record.text}（模拟润色）` } };
    throw new Error("素材已不存在");
  },
};
