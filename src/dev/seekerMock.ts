// Seeker in the browser mock: fictional materials (林小柚, Chinese and English), a simulated transfer/
// preview, templates, exports (no file is written), the draft's job and knowledge/'s companies
// (matched by name). Each language has its own draft (Rust seeker.rs). Rater's question bank (rater.rs)
// lives here too: its rules are Seeker records; 卡片练习 (practice.rs) keeps its rounds in `practice`.
import type { RaterAsk, RaterBank, RaterCard, RaterInterviews, RaterOptions, RaterPractice, RaterPrep, RaterPrepAsk, RaterQuestion, RaterReview, RaterRound, RaterStart, RaterTally } from "@/modules/picky/assistants/raterApi";
import type { SeekerCatalog, SeekerCompany, SeekerCvFile, SeekerDesk, SeekerDraft, SeekerEntry, SeekerExport, SeekerImport, SeekerJobCompany, SeekerLang, SeekerRecord, SeekerSelection, SeekerSettings, SeekerWorkspace, SourceChange } from "@/modules/picky/assistants/seekerApi";

const HEADER = ["工作邮箱", "联系电话", "个人网站", "工作地点"];
const HEADER_EN = ["Location", "Email", "Phone", "GitHub"];
const NAMES = ["姓名", "Name"];
const SUMMARY = "摘要";
const ORDER_EN = [SUMMARY, "技能", "教育", "项目", "工作", "证书"];
/** A rule's text as points (Rust catalog.rs `split_points`, loosely): a list item, a table row, a sentence each. */
const splitPoints = (text: string) => {
  const out: string[] = [];
  let head = true;
  for (const line of text.split("\n").map((l) => l.trim())) {
    if (!line) { head = true; continue; }
    if (line.startsWith("|")) {
      const cells = line.replace(/^\||\|$/g, "").split("|").map((c) => c.trim());
      if (!head && !cells.every((c) => /^[-: ]+$/.test(c))) out.push(cells.filter(Boolean).join("："));
      head = false;
      continue;
    }
    head = true;
    const bare = line.replace(/^#+\s*/, "");
    const item = bare.match(/^(?:[-*•]\s|\d+(?:\.\s?|、))(.*)$/);
    if (item) out.push(item[1].trim());
    else out.push(...(bare.match(/[^。！？]+[。！？]?/g) ?? []).map((s) => s.trim()));
  }
  return out.filter(Boolean).map((text, i) => ({ id: `p${i + 1}`, text }));
};
/** An older record in today's shape (catalog.rs `normalize`): a rule's text as points, a 个人约定 as a rule of one point. */
const normalize = (r: SeekerRecord): SeekerRecord => {
  if ((r.kind as string) === "decision") return { ...r, kind: "rule", points: r.text.trim() ? [{ id: "p1", text: r.text.trim() }] : [], text: "" };
  if (r.kind === "rule" && r.text.trim()) return { ...r, points: r.points.length ? r.points : splitPoints(r.text), text: "" };
  return r;
};
const rec = (r: Partial<SeekerRecord> & { id: string; title: string }): SeekerRecord => normalize({ kind: "material", category: "项目", text: "", time: "", role: "", location: "", points: [], fields: [], stack: "", material: null, materialEn: null, lang: "cn", source: null, revision: 1, enabled: true, ...r });
/** 技术栈 then the points (catalog.rs `choosable`). */
const choosable = (r: SeekerRecord) => [...(r.stack.trim() ? [{ id: "技术栈", text: `${r.lang === "en" ? "Tech Stack: " : "技术栈："}${r.stack.trim()}` }] : []), ...r.points];
const pts = (...texts: string[]) => texts.map((text, i) => ({ id: text.match(/^\*\*(.+?)\*\*/)?.[1] ?? (text.startsWith("技术栈") ? "技术栈" : /^[A-Z]/.test(text) ? String(i + 1) : text), text }));
const fields = (pairs: string[][]) => pairs.map(([label, value], i) => ({ id: `f${i + 1}`, label, value, visible: true }));
const sample = (): SeekerCatalog => ({
  records: [
    rec({ id: "r-profile", kind: "profile", category: "基本信息", title: "基本信息", source: "cn-modules/1 - 基本信息.md#基本信息:1", fields: fields([["姓名", "林小柚"], ["工作地点", "上海"], ["工作邮箱", "work@example.com"], ["个人邮箱", "me@example.com"], ["联系电话", "+86 100 0000 0000"], ["GitHub", "github.com/example"], ["个人网站", "example.com"]]) }),
    rec({ id: "r-edu", category: "教育", title: "示例大学 · 信息学院", role: "数据科学 · 硕士", time: "2025.07 - 至今", points: pts("**主修课程**：机器学习、数据库") }),
    rec({ id: "r-skill", category: "技能", title: "技能证书", points: pts("**编程语言**：Python、SQL", "**语言能力**：英语六级") }),
    rec({ id: "r-notes", title: "柚子笔记", role: "独立开发", text: "读书笔记整理工具", time: "2026.03 - 至今", stack: "Rust、SQLite", points: pts("**数据整理**：把零散笔记合成卷", "**检索改进**：按意思搜索") }),
    rec({ id: "r-orange", title: "橙子识别", role: "课程项目", text: "水果分类模型", time: "2025.10 - 2025.12", location: "上海", points: pts("**模型迭代**：两轮调参，准确率 0.91", "**类别均衡**：重采样处理样本失衡") }),
    rec({ id: "r-work", category: "工作", title: "示例证券 · 研究所", role: "研究部实习生", time: "2024.01 - 2024.02", location: "杭州", points: pts("**行业研究**：整理研报", "**数据分析**：写脚本汇总收益") }),
    rec({ id: "r-en-profile", lang: "en", kind: "profile", category: "基本信息", title: "Contact", source: "en-modules/2-CONTACT.md#Contact:1", fields: fields([["Name", "LIN XIAOYOU"], ["Location", "Shanghai"], ["Email", "work@example.com"], ["Personal Email", "me@example.com"], ["Phone", "+86 100 0000 0000"], ["GitHub", "github.com/example"], ["LinkedIn", "linkedin.com/in/example"]]) }),
    rec({ id: "r-en-sum", lang: "en", category: SUMMARY, title: "Data Summary (Default)", text: "Data science student combining machine learning with tool building — from a fruit classifier to a notes tool used every day." }),
    rec({ id: "r-en-sum2", lang: "en", category: SUMMARY, title: "Agent Summary", text: "Student focused on LLM agents. Built a reading-notes agent end to end and shipped it." }),
    rec({ id: "r-en-skill", lang: "en", category: "技能", title: "Skills", points: pts("**Programming**: Python, SQL, Rust", "**Languages**: English, Chinese (Native)") }),
    rec({ id: "r-en-edu", lang: "en", category: "教育", title: "Example University", role: "MSc in Data Science", time: "Jul 2025 – Present", points: pts("Coursework: Machine Learning, Databases") }),
    rec({ id: "r-en-notes", lang: "en", title: "Pomelo Notes: Reading Notes Tool", role: "Sole Developer", time: "Mar 2026 – Present", stack: "Rust, SQLite", points: pts("Built a notes merger that combines scattered notes into volumes", "Added meaning search over every note", "Shipped a desktop build used daily", "Wrote the user guide") }),
    rec({ id: "r-en-orange", lang: "en", title: "Orange Classifier: Fruit CNN", role: "Team Leader", time: "Oct 2025 – Dec 2025", points: pts("Led two tuning rounds to reach 0.91 accuracy", "Corrected class imbalance with resampling") }),
    rec({ id: "r-en-third", lang: "en", title: "Shopping Cart Web App", role: "Backend Developer", time: "Sep 2025 – Nov 2025", points: pts("Built REST APIs with Spring Boot") }),
    rec({ id: "r-en-work", lang: "en", category: "工作", title: "Example Securities, Research", role: "Research Intern", time: "Jan 2024 – Feb 2024", points: pts("Summarised sell-side research into briefs", "Automated return analysis with Python") }),
    rec({ id: "r-en-cert", lang: "en", category: "证书", title: "Certifications", points: pts("**IELTS Academic – Overall 7.5** · May 2025", "**Example Certificate** Example Org · Jan 2026") }),
    rec({ id: "r-legacy", kind: "rule", category: "素材润色", title: "中文素材润色规则", source: "rules/cn-modules.md#中文素材润色规则:1", text: "（以前整份导入的规则）\n\n## 事实与职责\n\n- 不编造事实" }),
    rec({ id: "r-shared", lang: "", kind: "rule", category: "共用准则", title: "定位", text: "Seeker 是求职资料助手，负责维护简历素材、生成简历。" }),
    rec({ id: "r-rule", kind: "rule", category: "素材润色", title: "事实与职责", text: "- 保留已确认的事实、指标和职责边界\n- 区分负责、参与、协助" }),
    rec({ id: "r-format", kind: "rule", category: "素材润色", title: "格式", text: "- 技能、项目、工作条目采用「**加粗四字小标题**＋全角冒号＋正文」\n- 日期用 `YYYY.MM - YYYY.MM`" }),
    rec({ id: "r-review", kind: "rule", category: "整体审查", title: "审查边界与输出", text: "### 输出\n\n按「需处理／可选优化／待确认」分组，每项写位置、问题与依据、改进方向。" }),
    rec({ id: "r-en-scope", lang: "en", kind: "rule", category: "素材润色", title: "适用范围", text: "仅润色选中的英文素材，正文保持英文，说明用中文。" }),
    rec({ id: "r-en-review", lang: "en", kind: "rule", category: "整体审查", title: "产物、选材与事实", text: "| 检查项 | 要求 |\n|---|---|\n| Resume | 针对具体 JD，严格 1 页；通常选 1–2 个项目 |\n| CV | 2 页，必要时 3 页 |" }),
    rec({ id: "r-q-scope", kind: "rule", category: "题库维护", title: "适用范围", text: "用于用户指定范围内的题库补充或润色，目标是形成可直接口述的答案。" }),
    rec({ id: "r-q-answer", kind: "rule", category: "题库维护", title: "答案要求", text: "- 一题一个问题，回答可在 60–90 秒内说完，过长则拆题\n- 使用短句、大白话" }),
    rec({ id: "r-i-prep", kind: "rule", category: "面试", title: "准备面试", text: "以实际投递的简历、对应题库、相关经历档案及公司／岗位资料为输入。" }),
    rec({ id: "r-i-review", kind: "rule", category: "面试", title: "面试复盘", text: "记录实际问题与本人实际回答、面试官明确反馈，再对照准备内容总结命中与遗漏。" }),
    rec({ id: "r-f1", kind: "fact", category: "个人事实", title: "到岗时间", text: "预计 2027 年 4 月之后可到岗，具体日期未定" }),
    rec({ id: "r-f2", kind: "fact", category: "个人事实", title: "语言成绩", text: "英语六级" }),
    rec({ id: "r-f3", kind: "fact", category: "中文素材口径", title: "职位名称", text: "「研究部实习生」为已确认职位名称，不重新提案" }),
    rec({ id: "r-dossier", kind: "fact", category: "经历档案", title: "橙子识别 · 水果分类模型", material: "r-orange", materialEn: "r-en-orange", text: "## 项目概览\n\n- 2025.10–2025.12，3 人课程项目，四类水果图像分类。\n\n## 我的定位和职责范围\n\n- 负责两轮调参和类别均衡。\n\n## 记录边界\n\n- 测试集与验证集的指标分开记。" }),
    rec({ id: "r-apply", kind: "application", category: "网申", title: "为什么选择我们", text: "示例答案。" }),
  ],
  revisions: [],
  groups: [],
  settings: { selectionModel: null, reviewModel: null, thinking: false, cloudAllowed: false },
});
/** knowledge/ of the mock Seeker folder: one folder per company; names as 招聘索引.md would give them. */
const COMPANIES: { folder: string; auto: string; names: string[] }[] = [
  { folder: "seek-boc", auto: "seek-boc", names: ["boc"] },
  { folder: "seek-bytedance", auto: "字节跳动", names: ["bytedance", "字节跳动"] },
  { folder: "seek-keyreply", auto: "seek-keyreply", names: ["keyreply"] },
  { folder: "seek-tencent", auto: "腾讯", names: ["tencent", "腾讯"] },
];
const nameKey = (s: string) => s.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
/** Names the user gave in 设置 (Rust company.rs `named`): shown and matched first. */
let companyNames: Record<string, string> = {};
const companyList = (): (SeekerCompany & { names: string[] })[] => COMPANIES.map((c) => { const own = companyNames[c.folder]; return own ? { folder: c.folder, name: own, auto: c.auto, names: [nameKey(own), ...c.names] } : { folder: c.folder, name: c.auto, auto: c.auto, names: c.names }; });
const shownCompanies = () => companyList().map(({ folder, name, auto }) => ({ folder, name, auto }));
const matched = (company: string) => {
  const key = nameKey(company);
  const hits = companyList().flatMap((c) => c.names.filter((n) => key.includes(n) || (key.length >= 2 && n.includes(key))).map((n) => ({ folder: c.folder, len: n.length })));
  const best = Math.max(0, ...hits.map((h) => h.len));
  const top = [...new Set(hits.filter((h) => h.len === best).map((h) => h.folder))];
  return top.length === 1 ? top[0] : null;
};
let kbAdded = false;
let cvAdded = false;
/** cv/ and material/ of the mock Seeker folder (made-up names; Rust seeker/files.rs scans the real one). */
const CV = [
  { name: "cn/林小柚-简历-2026-10-20.docx", series: "林小柚-简历", version: null, export: true },
  { name: "cn/seek-it-cn-template-v2.docx", series: "seek-it-cn-template", version: 2, template: true },
  { name: "cn/seek-it-cn-template-v1.docx", series: "seek-it-cn-template", version: 1 },
  { name: "en/seek-it-keyreply-resume-v3.docx", series: "seek-it-keyreply-resume", version: 3 },
  { name: "en/seek-it-keyreply-resume-v2.docx", series: "seek-it-keyreply-resume", version: 2 },
  { name: "en/seek-it-keyreply-resume-v1.docx", series: "seek-it-keyreply-resume", version: 1 },
  { name: "en/seek-it-tencent-cv-v1.pdf", series: "seek-it-tencent-cv", version: 1 },
];
const MATERIAL = [["certificates", "四级-林小柚.pdf"], ["certificates", "雅思-林小柚.pdf"], ["education", "成绩单-林小柚.pdf"], ["education", "学位证-林小柚.pdf"], ["experience", "示例证券-实习证明.pdf"], ["profile", "证件照.jpg"]];
/** cv/'s files: the made-up ones and Pomelo's exports saved under cv/ (Rust files.rs `scan`). */
const cvFiles = (): SeekerCvFile[] => {
  const folder = seekerMock.workspace.folder;
  if (!folder) return [];
  const day = Math.floor(Date.now() / 1000);
  const exports = seekerMock.workspace.output.exports.filter((e) => e.path.startsWith(`${folder}/cv/`));
  const made = exports.map((e, i) => { const name = e.path.slice(folder.length + 4); return { path: e.path, name, lang: (name.startsWith("en/") ? "en" : "cn") as SeekerLang, series: name.split("/").pop()!.replace(/\.docx$/i, "").replace(/[-_ ]v\d+$/i, ""), version: null, modified: day - i, size: 30000, template: false, export: true, exportId: e.id, jobs: [] }; });
  return [...made, ...CV.map((c, i) => ({ path: `${folder}/cv/${c.name}`, name: c.name, lang: (c.name.startsWith("cn/") ? "cn" : "en") as SeekerLang, series: c.series, version: c.version, modified: day - (i + 1) * 86400 * 9, size: 48000 + i * 1300, template: !!c.template, export: !!c.export, exportId: null, jobs: c.series.includes("keyreply") ? [] : c.series.includes("tencent") ? ["腾讯 · 后台开发"] : [] }))];
};
const associate = (path: string, jd: string, jobId: number) => {
  const output = seekerMock.workspace.output;
  const job = seekerMock.job(jobId);
  if (!job) throw new Error("岗位不存在，请重新选择");
  const same = (l: { job: { id: number; createdAt: string } }) => l.job.id === job.id && l.job.createdAt === job.createdAt;
  for (const link of output.links) if (same(link)) link.current = false;
  const previous = output.links.find((l) => same(l) && l.path === path);
  if (previous) previous.current = true;
  else output.links.unshift({ id: crypto.randomUUID(), path, job: { id: job.id, company: job.company, position: job.position, location: job.location, link: job.link, note: job.note, createdAt: job.createdAt }, jd, createdAt: new Date().toISOString(), current: true });
};
const MOCK_MATERIAL_GROUPS = ["教育", "技能", "项目", "工作", "证书", "摘要"];
const MOCK_RULE_GROUPS = ["共用准则", "素材润色", "整体审查", "写作规则", "题库维护", "面试", "简历题", "通用题", "突击题"];
const mockPrompt = (key: string, label: string, groups: string[], lang: string) => {
  const rules = catalog().records.filter((r) => r.enabled && r.kind === "rule" && groups.includes(r.category) && (r.lang === "" || (r.lang ?? "cn") === lang));
  // # 组, ## 标题, points numbered (Rust catalog.rs `rules_markdown`)
  const text = groups.map((g) => [g, rules.filter((r) => r.category === g)] as const).filter(([, rs]) => rs.length).map(([g, rs]) => `# ${g}\n\n${rs.map((r) => `## ${r.title}\n${r.points.map((p, i) => `${i + 1}. ${p.text}`).join("\n")}`).join("\n\n")}`).join("\n\n");
  return { key, label, groups, system: `（模拟）你是用户的求职写作助手。这里是「${label}」的说明：先说做什么、输出什么格式，再附上规则。\n\n规则：\n${text || "（这一项还没有规则）"}` };
};
const cvText = (name: string) => name.includes("keyreply") ? `LIN XIAOYOU\nProfile Summary\nData science student who builds tools.\nProjects\nPomelo Notes — semantic search over reading notes${name.includes("v3") ? "\nTop-5 hit rate from 54% to 81% on 120 test questions" : "\nImproved search quality"}` : `林小柚\n项目经历\n柚子笔记｜独立开发\n- 检索改进：按意思搜索`;

const question = (q: Partial<RaterQuestion> & { id: string; question: string }): RaterQuestion => ({ category: "通用题", answer: "", tags: [], material: null, job: null, from: null, revision: 1, ...q });
const sampleBank = (): RaterBank => ({
  questions: [
    question({ id: "q-intro", question: "自我介绍（1 分钟）", answer: "我是林小柚，示例大学数据科学硕士在读，做过读书笔记整理工具和水果分类模型两个项目，也在示例证券研究所实习过。", tags: ["必背"] }),
    question({ id: "q-why", question: "为什么投这个岗位？", answer: "示例答案：岗位要的数据分析和工程能力，我在两个项目里都用过。" }),
    question({ id: "q-orange", category: "简历题", material: "r-orange", question: "橙子识别里你具体负责什么？哪些是队友做的？", answer: "我负责两轮调参和类别均衡，数据清洗是队友做的。" + "具体来说，第一轮调参把学习率和数据增强的组合都试了一遍，".repeat(16), tags: ["必背"] }),
    question({ id: "q-notes", category: "简历题", material: "r-notes", question: "为什么用 SQLite 而不是别的数据库？", answer: "" }),
    question({ id: "q-tencent", category: "突击题", job: { id: -1, createdAt: "", label: "腾讯 · 后台开发" }, from: "q-why", question: "为什么投腾讯的后台开发？", answer: "示例答案：腾讯的后台规模大，我想在高并发的场景里练工程能力。" }),
  ],
  revisions: [],
});
/** 400 字 in 90 s: a CJK character one 字, a Latin word or number two (rater.rs `seconds`). */
const seconds = (text: string) => Math.ceil((((text.match(/[\u3400-\u9fff\uf900-\ufaff]/g) ?? []).length + 2 * (text.match(/[A-Za-z0-9]+(?:['’.\-][A-Za-z0-9]+)*/g) ?? []).length) * 90) / 400);
let raterOptions: RaterOptions = { model: null, thinking: false };
/** The built-in workflows (Rust rater/qbank.rs `default_workflow`), shortened: their points. */
const WORKFLOWS: [string, string[]][] = [["简历题", ["面试官照着简历问，要有问必答：按每段经历各出几道。", "只用素材和经历档案里有的事实。"]], ["通用题", ["题型：自我介绍（1 分钟、3 分钟两版）、为什么投、优缺点、行为题、职业规划、反问。"]], ["突击题", ["每个岗位第一次出 8～10 道，再把已有的题按这家公司改写几道。"]]];
const bankView = (): RaterBank => structuredClone({ ...seekerMock.rater, seconds: Object.fromEntries(seekerMock.rater.questions.map((q) => [q.id, seconds(q.answer)])), practice: tallies(seekerMock.practice) });

// ---- 卡片练习 (Rust rater/practice.rs, loosely) ----
const KNOW = "会";
const UNSURE = "不熟";
const MODES = ["自己挑", "按 JD", "随机"];
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
/** Each question's record from the rounds (follow-ups don't count). */
const tallies = (rounds: RaterRound[]): Record<string, RaterTally> => {
  const marks = rounds.flatMap((r) => r.cards.filter((c) => !c.follow && c.mark && c.markedAt).map((c) => ({ at: c.markedAt!, q: c.question, m: c.mark! })));
  marks.sort((a, b) => (a.at < b.at ? -1 : a.at > b.at ? 1 : 0));
  const out: Record<string, RaterTally> = {};
  for (const { at, q, m } of marks) {
    const t = (out[q] ??= { know: 0, unsure: 0, last: null, lastMark: "", streak: 0 });
    if (m === KNOW) { t.know++; t.streak++; } else { t.unsure++; t.streak = 0; }
    t.last = at; t.lastMark = m;
  }
  return out;
};
const rankOf = (t?: RaterTally) => (!t ? 1 : t.lastMark === UNSURE ? 0 : t.streak >= 2 ? 2 : 1);
const shuffle = <T,>(v: T[]) => { for (let i = v.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [v[i], v[j]] = [v[j], v[i]]; } return v; };
const practiceView = (): RaterPractice => {
  const all = seekerMock.practice;
  const count = (r: RaterRound, f: (c: RaterCard) => boolean) => r.cards.filter(f).length;
  return structuredClone({
    running: all.find((r) => !r.endedAt) ?? null,
    rounds: [...all].reverse().filter((r) => r.endedAt).map((r) => ({ id: r.id, mode: r.mode, job: r.job?.label ?? null, startedAt: r.startedAt, endedAt: r.endedAt, cards: count(r, (c) => !c.follow), follows: count(r, (c) => c.follow), know: count(r, (c) => c.mark === KNOW), unsure: count(r, (c) => c.mark === UNSURE), left: count(r, (c) => !c.mark) })),
  });
};
const roundOf = (id: unknown) => {
  const r = seekerMock.practice.find((x) => x.id === id);
  if (!r) throw new Error("这一轮已经删除了");
  return r;
};
const currentOf = (r: RaterRound) => (r.endedAt ? -1 : r.cards.findIndex((c) => !c.mark));
const onScreen = (id: unknown, card: unknown) => {
  const r = roundOf(id);
  const i = currentOf(r);
  if (i < 0) throw new Error("这一轮已经结束了");
  if (i !== card) throw new Error("卡片对不上了，请重新打开这一轮");
  return r;
};
const modelCheck = () => {
  if (!raterOptions.model) throw new Error("请先在 Rater 里选模型");
  if (raterOptions.model.startsWith("cloud:") && !catalog().settings.cloudAllowed) throw new Error("Seeker 资料（Rater 也照它）还没允许发给云端，请在 Seeker 的设置里打开，或选本地模型");
};
const canFollow = (r: RaterRound, card: number) => {
  const k = r.cards[card];
  if (currentOf(r) !== card || !k) throw new Error("卡片对不上了，请重新打开这一轮");
  if (k.asked) throw new Error("这张卡已经追问过了");
  if (r.cards.filter((c) => c.follow && c.question === k.question).length >= 2) throw new Error("这道题这一轮已经追问了 2 次");
};
const sameJob = (q: RaterQuestion, job: Job) => q.job != null && (q.job.id === job.id || q.job.label === (job.position ? `${job.company} · ${job.position}` : job.company));
const interviewsView = (): RaterInterviews => structuredClone({ preps: [...seekerMock.interviews.preps].sort((a, b) => b.createdAt.localeCompare(a.createdAt)), reviews: [...seekerMock.interviews.reviews].sort((a, b) => b.createdAt.localeCompare(a.createdAt)) });
const jobLink = (id: number) => { const j = seekerMock.job(id); if (!j) throw new Error("岗位不存在，请重新选择"); return { id: j.id, createdAt: j.createdAt, label: j.position ? `${j.company} · ${j.position}` : j.company }; };
const interviewHandlers: Record<string, (a: Args) => unknown> = {
  picky_rater_interviews: interviewsView,
  picky_rater_prep: async (a) => {
    modelCheck();
    const ask = a.ask as RaterPrepAsk;
    const job = jobLink(ask.job);
    await wait(1200);
    const bank = seekerMock.rater.questions;
    const picks = [...bank.filter((q) => q.category === "通用题"), ...bank.filter((q) => q.category === "简历题"), ...bank.filter((q) => q.category === "突击题" && q.job?.label === job.label)];
    const sheet = `# ${job.label} 速查卡（模拟）\n\n## 岗位要点\n- 高并发后台 → 柚子笔记的本地检索（待补：并发数据）\n- Go / C++ → （待补）\n\n## 自我介绍\n- 数据科学硕士在读；两个项目、一段实习\n\n## 经历速记\n- 柚子笔记｜本人负责全部代码｜命中率 54% → 81%｜测试集是自己攒的 120 条\n- 橙子识别｜调参和类别均衡｜0.91｜测试集只有 60 张\n\n## 高频题\n${picks.slice(0, 4).map((q) => `- ${q.question}`).join("\n")}\n\n## 可能的追问\n- 81% 是怎么算的？ → 前 5 条命中（待补：标注方法）\n\n## 反问\n- 团队现在最想解决的问题是什么？`;
    return seekerMock.wrap("准备面试", () => {
      seekerMock.interviews.preps.push({ id: crypto.randomUUID(), job, resume: "中文草稿", lang: ask.lang, form: ask.form.trim(), request: ask.request.trim(), questions: picks.map((q) => q.id), follows: [{ about: "柚子笔记", category: "简历题", question: "（模拟追问）命中率是在什么数据上算的？", answer: "在我自己攒的 120 条测试问题上（待补：问题怎么来的）", saved: null }, { about: "JD：高并发", category: "突击题", question: "（模拟追问）你处理过多大的并发？", answer: "（待补：资料里没有并发的经历）", saved: null }], sheet, advice: "（模拟模型）先把自我介绍和柚子笔记背熟，JD 里的高并发缺证据。", model: "模拟模型", note: "", createdAt: new Date().toISOString() });
      return interviewsView();
    });
  },
  picky_rater_prep_save: (a) => {
    const input = a.prep as RaterPrep;
    const p = seekerMock.interviews.preps.find((x) => x.id === input.id);
    if (!p) throw new Error("这份准备已经删除了");
    if (!input.sheet.trim()) throw new Error("速查卡不能是空的");
    p.sheet = input.sheet;
    return interviewsView();
  },
  picky_rater_prep_keep: (a) => {
    const p = seekerMock.interviews.preps.find((x) => x.id === a.id);
    const f = p?.follows[a.index as number];
    if (!p || !f) throw new Error("没有这道追问");
    if (f.saved) throw new Error("已经存进题库了");
    const id = crypto.randomUUID();
    seekerHandlers.picky_rater_save({ question: question({ id, category: f.category, question: f.question, answer: f.answer, job: f.category === "突击题" ? p.job : null, revision: 0 }) });
    f.saved = id;
    return interviewsView();
  },
  picky_rater_review_save: (a) => {
    const r = structuredClone(a.review as RaterReview);
    r.items = r.items.filter((x) => x.question.trim() || x.answer.trim() || x.feedback.trim());
    if (r.items.some((x) => !x.question.trim())) throw new Error("每一条都要写问题");
    if (!r.date) throw new Error("请填写面试日期");
    if (!r.id) {
      Object.assign(r, { id: crypto.randomUUID(), job: jobLink(a.jobId as number), createdAt: new Date().toISOString(), analysis: null });
      seekerMock.interviews.reviews.push(r);
    } else {
      const old = seekerMock.interviews.reviews.find((x) => x.id === r.id);
      if (!old) throw new Error("这次复盘已经删除了");
      Object.assign(old, { ...r, job: old.job, createdAt: old.createdAt, analysis: old.analysis });
    }
    return interviewsView();
  },
  picky_rater_review_compare: async (a) => {
    modelCheck();
    const r = seekerMock.interviews.reviews.find((x) => x.id === a.id);
    if (!r) throw new Error("这次复盘已经删除了");
    if (!r.items.length) throw new Error("先记下面试里问了什么");
    await wait(1000);
    if (r.feedback.includes("失败测试")) throw new Error("模拟的失败：模型回复意外中断。");
    return seekerMock.wrap("复盘对照", () => {
      r.analysis = { model: "模拟模型", prep: (a.prep as string | null) ?? null, summary: "（模拟模型）自我介绍和项目题准备到了，追问数据来源时卡住。", hits: r.items.map((_, i) => i).filter((i) => i % 2 === 0).map((i) => ({ asked: i, text: "准备里的自我介绍" })), misses: r.items.map((_, i) => i).filter((i) => i % 2 === 1).map((i) => ({ asked: i, text: "没准备到：数据从哪来讲不清" })), questions: [{ about: "", category: "突击题", question: "（模拟）你们的数据是怎么来的？", answer: "（待补：数据来源）", saved: null }], rules: [{ use: "面试", title: "（模拟）先说数据口径", text: "被问到数字时先说在什么数据上、怎么算的。", why: "这次两次被追问口径", adopted: null }] };
      return interviewsView();
    });
  },
  picky_rater_review_keep: (a) => {
    const r = seekerMock.interviews.reviews.find((x) => x.id === a.id);
    const f = r?.analysis?.questions[a.index as number];
    if (!r || !f) throw new Error("没有这道题");
    if (f.saved) throw new Error("已经存进题库了");
    const id = crypto.randomUUID();
    seekerHandlers.picky_rater_save({ question: question({ id, category: f.category, question: f.question, answer: f.answer, job: f.category === "突击题" ? r.job : null, revision: 0 }) });
    f.saved = id;
    return interviewsView();
  },
  picky_rater_review_rule: (a) => {
    const r = seekerMock.interviews.reviews.find((x) => x.id === a.id);
    const idea = r?.analysis?.rules[a.index as number];
    if (!r || !idea) throw new Error("没有这条");
    if (idea.adopted) throw new Error("已经加进规则了");
    // a point of the rule with that title in that group, else a new rule (interview.rs `adopt_rule`)
    const same = catalog().records.find((x) => x.kind === "rule" && x.category === idea.use && x.title.trim() === idea.title.trim());
    const point = { id: crypto.randomUUID(), text: idea.text };
    if (same) { same.points.push(point); same.revision += 1; idea.adopted = same.id; return interviewsView(); }
    const id = crypto.randomUUID();
    catalog().records.push(rec({ id, kind: "rule", category: idea.use, title: idea.title, points: [point] }));
    idea.adopted = id;
    return interviewsView();
  },
  picky_rater_interview_delete: (a) => {
    const before = seekerMock.interviews.preps.length + seekerMock.interviews.reviews.length;
    seekerMock.interviews.preps = seekerMock.interviews.preps.filter((x) => x.id !== a.id);
    seekerMock.interviews.reviews = seekerMock.interviews.reviews.filter((x) => x.id !== a.id);
    if (seekerMock.interviews.preps.length + seekerMock.interviews.reviews.length === before) throw new Error("已经删除了");
    return interviewsView();
  },
  picky_rater_prep_word_place: (a) => {
    const p = seekerMock.interviews.preps.find((x) => x.id === a.id);
    if (!p) throw new Error("这份准备已经删除了");
    return { folder: "/Users/mock/Documents", name: `速查卡 ${p.job.label.replace(" · ", " ")} ${new Date().toISOString().slice(0, 10)}` };
  },
  picky_rater_prep_word: (a) => `${a.folder}/${String(a.name).replace(/\.docx$/, "")}.docx`,
};
const practiceHandlers: Record<string, (a: Args) => unknown> = {
  ...interviewHandlers,
  picky_rater_practice: practiceView,
  picky_rater_round: (a) => structuredClone(roundOf(a.id)),
  picky_rater_round_start: (a) => {
    const st = a.start as RaterStart;
    if (seekerMock.practice.some((r) => !r.endedAt)) throw new Error("上一轮还没停下");
    if (!MODES.includes(st.mode)) throw new Error("未知的出卡方式");
    const job = st.mode === "按 JD" && st.job != null ? seekerMock.job(st.job) : undefined;
    if (st.mode === "按 JD" && !job) throw new Error("按 JD 要先选一个岗位");
    const bank = seekerMock.rater.questions;
    const t = tallies(seekerMock.practice);
    let picked = st.mode === "随机" ? [...bank] : [...new Set(st.questions)].map((id) => { const q = bank.find((x) => x.id === id); if (!q) throw new Error("选的题已经删除了，请重新选"); return q; });
    if (st.mode === "按 JD") picked = picked.map((q, i) => ({ q, i })).sort((x, y) => ["通用题", "简历题", "突击题"].indexOf(x.q.category) - ["通用题", "简历题", "突击题"].indexOf(y.q.category) || x.i - y.i).map((x) => x.q);
    else picked = shuffle(picked).map((q, i) => ({ q, i })).sort((x, y) => rankOf(t[x.q.id]) - rankOf(t[y.q.id]) || x.i - y.i).map((x) => x.q);
    if (st.mode === "随机") picked = picked.slice(0, st.count || 10);
    if (!picked.length) throw new Error(bank.length ? "至少选一道题" : "题库还是空的");
    const card = (q: RaterQuestion): RaterCard => ({ question: q.id, text: q.question, category: q.category, follow: false, reference: "", saved: null, mark: null, markedAt: null, typed: "", asked: false });
    seekerMock.practice.push({ id: crypto.randomUUID(), mode: st.mode, job: job ? { id: job.id, createdAt: job.createdAt, label: job.position ? `${job.company} · ${job.position}` : job.company } : null, advice: st.mode === "按 JD" ? st.advice.trim() : "", cards: picked.map(card), startedAt: new Date().toISOString(), endedAt: null, feedback: null });
    return practiceView();
  },
  picky_rater_mark: (a) => {
    if (a.mark !== KNOW && a.mark !== UNSURE) throw new Error("只能是「会」或「不熟」");
    const r = onScreen(a.id, a.card);
    const k = r.cards[a.card as number];
    const at = new Date().toISOString();
    Object.assign(k, { mark: a.mark, markedAt: at, typed: String(a.typed ?? "").trimEnd() });
    if (currentOf(r) < 0) r.endedAt = at;
    return practiceView();
  },
  picky_rater_stop: (a) => {
    const r = roundOf(a.id);
    if (r.endedAt) throw new Error("这一轮已经结束了");
    if (r.cards.every((c) => !c.mark)) seekerMock.practice = seekerMock.practice.filter((x) => x.id !== r.id);
    else r.endedAt = new Date().toISOString();
    return practiceView();
  },
  picky_rater_rounds_delete: (a) => {
    const ids = a.ids as string[];
    seekerMock.practice = seekerMock.practice.filter((x) => !ids.includes(x.id));
    return practiceView();
  },
  picky_rater_round_delete: (a) => {
    roundOf(a.id);
    seekerMock.practice = seekerMock.practice.filter((x) => x.id !== a.id);
    return practiceView();
  },
  picky_rater_pick: async (a) => {
    modelCheck();
    const job = seekerMock.job(a.job as number);
    if (!job) throw new Error("岗位不存在，请重新选择");
    const bank = seekerMock.rater.questions;
    if (!bank.length) throw new Error("题库还是空的");
    await wait(700);
    const of = (cat: string, n: number) => bank.filter((q) => q.category === cat && (cat !== "突击题" || sameJob(q, job))).slice(0, n).map((q) => q.id);
    return { questions: [...of("通用题", 2), ...of("简历题", 3), ...of("突击题", 3)], advice: "（模拟模型）先自我介绍，再简历题，最后突击题。", model: "模拟模型", note: "" };
  },
  picky_rater_follow: async (a) => {
    modelCheck();
    const r = roundOf(a.id);
    canFollow(r, a.card as number);
    await wait(900);
    if (String(a.typed ?? "").includes("失败测试")) throw new Error("模拟的失败：模型没有回复。");
    return seekerMock.wrap("追问", () => {
      const live = onScreen(a.id, a.card);
      canFollow(live, a.card as number);
      const k = live.cards[a.card as number];
      Object.assign(k, { asked: true, typed: String(a.typed ?? "").trimEnd() });
      live.cards.splice((a.card as number) + 1, 0, { question: k.question, text: `（模拟追问 ${live.cards.filter((c) => c.follow && c.question === k.question).length + 1}）${seekerMock.rater.questions.find((q) => q.id === k.question)?.question ?? k.text}——这一步具体是你本人做的吗？`, category: k.category, follow: true, reference: "（模拟参考答案）是我本人做的：先……再……（待补：用的数据和结果）", saved: null, mark: null, markedAt: null, typed: "", asked: false });
      return practiceView();
    });
  },
  picky_rater_keep: (a) => {
    const r = roundOf(a.id);
    const k = r.cards[a.card as number];
    if (!k?.follow) throw new Error("这不是追问卡");
    if (k.saved) throw new Error("这张追问卡已经存进题库了");
    const root = seekerMock.rater.questions.find((q) => q.id === k.question);
    const id = crypto.randomUUID();
    seekerHandlers.picky_rater_save({ question: question({ id, category: k.category, question: k.text, answer: k.reference, material: root?.material ?? null, job: root?.job ?? (k.category === "突击题" ? r.job : null), revision: 0 }) });
    k.saved = id;
    return structuredClone(r);
  },
  picky_rater_comment: async (a) => {
    modelCheck();
    const r = roundOf(a.id);
    if (!r.endedAt) throw new Error("这一轮还没结束");
    await wait(1100);
    if (r.cards.some((c) => c.typed.includes("失败测试"))) throw new Error("模拟的失败：模型回复意外中断。");
    return seekerMock.wrap("练习反馈", () => {
      const live = roundOf(a.id);
      live.feedback = { model: "模拟模型", summary: "（模拟模型）这一轮整体顺，开头的结论都说得出来。", good: ["（模拟）自我介绍一句话就说清了在读和两个项目"], gaps: ["（模拟）柚子笔记的命中率怎么测的讲不清（待补：测试集从哪来）"], notes: live.cards.map((c, i) => ({ c, i })).filter(({ c }) => c.typed.trim()).map(({ i }) => ({ card: i, text: "（模拟点评）先说结论，再说本人做了哪一步；数字说清楚在什么数据上。" })) };
      return structuredClone(live);
    });
  },
};

type Job = { id: number; company: string; position: string | null; location: string | null; link: string | null; note: string | null; createdAt: string };
const emptyDesk = (): SeekerDesk => ({ draft: null, updatedAt: null, baseline: null });
/** `job`: 投石问路's jobs (mockIpc sets it). */
export const seekerMock: { workspace: SeekerWorkspace; rater: RaterBank; practice: RaterRound[]; interviews: RaterInterviews; job: (id: number) => Job | undefined; wrap: <T>(label: string, f: () => T) => T } = { rater: sampleBank(), practice: [], interviews: { preps: [], reviews: [] }, wrap: (_label, f) => f(), workspace: { folder: "", ...emptyDesk(), en: emptyDesk(), catalog: sample(), output: { template: null, templateEn: null, exports: [], links: [] } }, job: () => undefined };
const catalog = () => seekerMock.workspace.catalog;
const langOf = (a: { lang?: unknown }): SeekerLang => (a.lang === "en" ? "en" : "cn");
const langOfDraft = (d: SeekerDraft | null | undefined): SeekerLang => (d?.lang === "en" ? "en" : "cn");
/** The Chinese desk is the workspace itself (its names from before English). */
const desk = (lang: SeekerLang): SeekerDesk => (lang === "en" ? (seekerMock.workspace.en ??= emptyDesk()) : seekerMock.workspace);
const isStack = (text: string) => /^(技术栈|tech stack)/i.test(text);
/** A development JD: 技术栈 chosen by default (Rust workflow.rs `dev_jd`). */
const devJd = (jd: string) => /开发|研发|工程师|程序员|后端|前端|全栈|developer|engineer|software|programmer|back-?end|front-?end|full[ -]stack/i.test(jd);
const warningsOf = (d: SeekerDraft | null) => {
  const c = catalog();
  const warnings: string[] = [];
  if (!d) return warnings;
  const count = (category: string) => d.sections.filter((s) => s.category === category).reduce((n, s) => n + s.entries.length, 0);
  if (langOfDraft(d) === "en") {
    if (d.form !== "cv" && count("项目") > 2) warnings.push(`Resume 建议 1–2 个项目，现在 ${count("项目")} 个`);
    for (const e of d.sections.filter((s) => s.category === "项目" || s.category === "工作").flatMap((s) => s.entries)) if (e.points.length > 3) warnings.push(`「${e.name}」${e.points.length} 条，每段最多 3 条`);
    if (count(SUMMARY) > 1) warnings.push("Summary 只留一条");
  } else for (const category of ["项目", "工作"]) if (count(category) > 3) warnings.push(`${category}经历 ${count(category)} 段，建议 3 段以内`);
  for (const e of d.sections.flatMap((s) => s.entries)) if (e.source && !c.records.some((r) => r.id === e.source!.recordId)) warnings.push(`「${e.name}」的素材已删除`);
  return warnings;
};
const view = () => {
  const c = catalog();
  const w = seekerMock.workspace;
  const choices = c.records.filter((r) => r.kind === "material" && r.enabled).map((r) => ({ recordId: r.id, pointIds: choosable(r).filter((p) => devJd(desk((r.lang ?? "cn") === "en" ? "en" : "cn").draft?.jd ?? "") || !isStack(p.text)).map((p) => p.id) }));
  const en = desk("en");
  return structuredClone({ ...w, warnings: warningsOf(w.draft), en: { ...en, warnings: warningsOf(en.draft) }, choices });
};
const save = (draft: SeekerDraft) => {
  const ids = [...draft.header.map((h) => h.id), ...draft.sections.flatMap((s) => [s.id, ...s.entries.flatMap((e) => [e.id, ...e.points.map((p) => p.id)])])];
  if (ids.some((id) => !id) || new Set(ids).size !== ids.length) throw new Error("简历中有空白或重复的块编号");
  const lang = langOfDraft(draft);
  if ((lang === "en") !== (draft.form === "resume" || draft.form === "cv")) throw new Error("简历的类型不对");
  const d = desk(lang);
  if (JSON.stringify(d.draft) !== JSON.stringify(draft)) { d.draft = structuredClone({ ...draft, lang }); d.updatedAt = new Date().toISOString(); }
  return view();
};
const entry = (r: SeekerRecord, pointIds: string[], id: string = crypto.randomUUID()): SeekerEntry => ({ id, time: r.time, name: r.title, role: r.role, location: r.location, note: r.text, points: choosable(r).filter((p) => pointIds.includes(p.id)).map((p) => ({ id: `${id}:${p.id}`, text: p.text })), source: { recordId: r.id, revision: r.revision, pointIds } });
const EN_TITLES: Record<string, string> = { [SUMMARY]: "Profile Summary", 技能: "Skills", 教育: "Education", 项目: "Projects", 工作: "Experience", 证书: "Certifications" };
const title = (category: string, lang: SeekerLang) => lang === "en" ? EN_TITLES[category] ?? category : category === "教育" ? "教育背景" : category === "技能" ? "专业技能" : `${category}经历`;
const materialIn = (r: SeekerRecord, lang: SeekerLang) => r.kind === "material" && r.enabled && (r.lang ?? "cn") === lang;
const all = (r: SeekerRecord) => choosable(r).map((p) => p.id);
const initial = (lang: SeekerLang): SeekerDraft => {
  const defaults = lang === "en" ? HEADER_EN : HEADER;
  const profile = catalog().records.find((r) => r.kind === "profile" && r.enabled && (r.lang ?? "cn") === lang);
  const value = (label: string) => profile?.fields.find((f) => f.label === label)?.value ?? "";
  const header = defaults.map((label, i) => ({ id: `header-${i}`, label, value: value(label), visible: true }));
  for (const f of profile?.fields ?? []) if (!NAMES.includes(f.label) && !defaults.includes(f.label)) header.push({ id: `header-${header.length}`, label: f.label, value: f.value, visible: false });
  const summary = lang === "en" ? catalog().records.find((r) => materialIn(r, lang) && r.category === SUMMARY) : undefined;
  const sections = [...(summary ? [{ id: crypto.randomUUID(), category: SUMMARY, title: title(SUMMARY, lang), entries: [entry(summary, [])] }] : []), { id: crypto.randomUUID(), category: "教育", title: title("教育", lang), entries: catalog().records.filter((r) => materialIn(r, lang) && r.category === "教育").map((r) => entry(r, all(r))) }];
  return { lang, ...(lang === "en" ? { form: "resume" as const } : {}), name: NAMES.map(value).find(Boolean) ?? "", header, sections, jd: "" };
};
const put = (raw: SeekerRecord) => {
  const input = normalize(raw.kind === "rule" ? { ...raw, points: raw.points.filter((p) => p.text.trim()) } : raw);
  if (!input.id || !input.title.trim()) throw new Error("请填写名称");
  if (input.kind === "rule" && !input.points.length) throw new Error("规则要有至少一个要点");
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
const jobCompany = (jobId: number): SeekerJobCompany => {
  const job = seekerMock.job(jobId);
  if (!job) throw new Error("岗位不存在，请重新选择");
  const chosen = seekerMock.workspace.jobCompanies?.find((c) => c.jobId === job.id && c.jobCreatedAt === job.createdAt)?.folder ?? null;
  return { companies: seekerMock.workspace.folder ? shownCompanies() : [], auto: matched(job.company), chosen };
};
const draftOf = (lang: SeekerLang) => structuredClone(desk(lang).draft);

type Args = Record<string, unknown>;
export const seekerHandlers: Record<string, (a: Args) => unknown> = {
  ...practiceHandlers,
  picky_rater: bankView,
  picky_rater_options: () => structuredClone(raterOptions),
  picky_rater_options_set: (a) => { raterOptions = structuredClone(a.options as RaterOptions); return structuredClone(raterOptions); },
  picky_rater_defaults: () => structuredClone(WORKFLOWS),
  picky_rater_suggest: (a) => {
    const ask = a.ask as RaterAsk;
    if (!raterOptions.model) throw new Error("请先在 Rater 里选模型");
    if (raterOptions.model.startsWith("cloud:") && !catalog().settings.cloudAllowed) throw new Error("Seeker 资料（Rater 也照它）还没允许发给云端，请在 Seeker 的设置里打开，或选本地模型");
    const bank = seekerMock.rater;
    const base = { kind: ask.kind, model: "模拟模型", note: "", advice: "（模拟模型）出了几道示例题。", questions: [] as RaterQuestion[], before: null as RaterQuestion | null, after: null as RaterQuestion | null };
    if (ask.kind === "润色") {
      const q = bank.questions.find((x) => x.id === ask.question);
      if (!q) throw new Error("这道题已经删除了");
      return { ...base, advice: "（模拟）按骨架理了一遍。", before: structuredClone(q), after: { ...structuredClone(q), answer: `（模拟润色）一句话结论：${q.answer}` } };
    }
    if (ask.kind === "突击题" && ask.job == null) throw new Error("突击题要先选一个岗位");
    const job = ask.job != null ? seekerMock.job(ask.job) : undefined;
    const mats = catalog().records.filter((r) => r.kind === "material" && r.enabled && (r.lang ?? "cn") === "cn" && (ask.materials.length ? ask.materials.includes(r.id) : ["项目", "工作"].includes(r.category)));
    const n = ask.count || 3;
    const questions: RaterQuestion[] = Array.from({ length: n }, (_, i) => {
      const m = ask.kind === "简历题" ? mats[i % Math.max(1, mats.length)] : undefined;
      return question({ id: crypto.randomUUID(), category: ask.kind, revision: 0, question: ask.kind === "简历题" ? `（模拟）${m?.title ?? "这段经历"}里你具体负责什么？（${i + 1}）` : ask.kind === "通用题" ? `（模拟）通用题 ${i + 1}：说说你的一个缺点` : `（模拟）${job?.company ?? "这家公司"}的 JD 要求，你怎么证明？（${i + 1}）`, answer: "（模拟）一句话结论 → 我做了什么 → 结果 → 局限。", material: m?.id ?? null, job: ask.kind === "突击题" && job ? { id: job.id, createdAt: job.createdAt, label: job.position ? `${job.company} · ${job.position}` : job.company } : null, from: ask.kind === "突击题" && i === 0 ? bank.questions.find((q) => q.category !== "突击题")?.id ?? null : null, tags: i === 0 ? ["必背"] : [] });
    });
    return { ...base, questions };
  },
  picky_rater_add: (a) => {
    const list = a.questions as RaterQuestion[];
    if (!list.length) throw new Error("至少勾上一道");
    for (const q of list) seekerHandlers.picky_rater_save({ question: q });
    return bankView();
  },
  picky_rater_seconds: (a) => seconds(String(a.text ?? "")),
  picky_rater_save: (a) => {
    const input = structuredClone(a.question as RaterQuestion);
    input.question = input.question.trim();
    if (!input.question) throw new Error("请填写问题");
    if (!["简历题", "通用题", "突击题"].includes(input.category)) throw new Error("未知的题目类别");
    input.tags = [...new Set(input.tags.map((t) => t.trim()).filter(Boolean))];
    const bank = seekerMock.rater;
    const prior = bank.questions.find((q) => q.id === input.id);
    if ((prior?.revision ?? 0) !== input.revision) throw new Error(prior ? "这道题刚刚改过，请重新打开" : "这道题已经删除了");
    if (prior && JSON.stringify({ ...prior }) === JSON.stringify({ ...input })) return bankView();
    if (prior) bank.revisions.unshift({ id: Math.max(0, ...bank.revisions.map((r) => r.id)) + 1, questionId: prior.id, body: structuredClone(prior), savedAt: new Date().toISOString() });
    const saved = { ...input, revision: (prior?.revision ?? 0) + 1 };
    const index = bank.questions.findIndex((q) => q.id === saved.id);
    if (index < 0) bank.questions.push(saved); else bank.questions[index] = saved;
    return bankView();
  },
  picky_rater_order: (a) => {
    const qs = seekerMock.rater.questions;
    const ids = a.ids as string[];
    const places = ids.map((id) => qs.findIndex((q) => q.id === id));
    if (places.some((i) => i < 0)) throw new Error("有题已经删除了，请刷新");
    const sorted = [...places].sort((x, y) => x - y);
    const moved = ids.map((id) => qs.find((q) => q.id === id)!);
    sorted.forEach((place, i) => { qs[place] = moved[i]; });
    return bankView();
  },
  picky_rater_delete_many: (a) => {
    for (const id of a.ids as string[]) seekerHandlers.picky_rater_delete({ id });
    return bankView();
  },
  picky_rater_delete: (a) => {
    const bank = seekerMock.rater;
    const prior = bank.questions.find((q) => q.id === a.id);
    if (!prior) throw new Error("这道题已经删除了");
    bank.revisions.unshift({ id: Math.max(0, ...bank.revisions.map((r) => r.id)) + 1, questionId: prior.id, body: structuredClone(prior), savedAt: new Date().toISOString() });
    bank.questions = bank.questions.filter((q) => q.id !== a.id);
    return bankView();
  },
  picky_seeker: view,
  picky_seeker_records_order: (a) => {
    const c = catalog();
    const ids = a.ids as string[];
    const places = ids.map((id) => c.records.findIndex((r) => r.id === id));
    if (places.some((i) => i < 0)) throw new Error("有资料已经删除了，请刷新");
    const sorted = [...places].sort((x, y) => x - y);
    const moved = ids.map((id) => c.records.find((r) => r.id === id)!);
    sorted.forEach((place, i) => { c.records[place] = moved[i]; });
    return view();
  },
  picky_seeker_records_enable: (a) => {
    for (const id of a.ids as string[]) {
      const r = catalog().records.find((x) => x.id === id);
      if (!r) throw new Error("有资料已经删除了，请刷新");
      if (r.enabled !== a.enabled) seekerHandlers.picky_seeker_record_save({ record: { ...structuredClone(r), enabled: a.enabled as boolean } });
    }
    return view();
  },
  picky_seeker_records_delete: (a) => {
    for (const id of a.ids as string[]) seekerHandlers.picky_seeker_record_delete({ id });
    return view();
  },
  // 分组改名 (Rust catalog.rs `rename_group`): a built-in group only shows the new name; the user's own moves its records
  picky_seeker_group_rename: (a) => {
    const kind = a.kind as "material" | "rule";
    const lang = kind === "rule" ? "" : String(a.lang);
    const from = String(a.from);
    const to = String(a.to).trim();
    if (!to || [...to].length > 30) throw new Error("分组名要 1～30 个字");
    const c = catalog();
    if ((kind === "material" ? MOCK_MATERIAL_GROUPS : MOCK_RULE_GROUPS).includes(from)) {
      c.groups = c.groups.filter((g) => !(g.kind === kind && g.lang === lang && g.category === from));
      if (to !== from) c.groups.push({ kind, lang, category: from, label: to });
    } else {
      for (const r of c.records) if (r.category === from && (kind === "material" ? r.kind === "material" && (r.lang ?? "cn") === lang : r.kind === "rule")) { r.category = to; r.revision += 1; }
    }
    return view();
  },
  // 发给模型的说明 (Rust model.rs / rater.rs `previews`): the groups each function reads, a made-up system message with the rules
  picky_seeker_prompts: (a) => {
    const lang = langOf(a);
    return [["select", "按 JD 推荐素材", "整体审查"], ["review", "审查整份简历", "整体审查"], ...(lang === "en" ? [["summary", "按 JD 改写 Summary", "整体审查"]] : []), ["polish", "润色要点", "素材润色"], ["material", "润色一条素材", "素材润色"]].map(([key, label, task]) => mockPrompt(key, label, ["共用准则", "写作规则", task, "个人范例"], lang));
  },
  picky_rater_prompts: () => [
    ...["简历题", "通用题", "突击题"].map((c) => mockPrompt(c, `补题 · ${c}`, ["共用准则", "题库维护", c], "cn")),
    mockPrompt("polish", "润色答案", ["共用准则", "题库维护", "简历题", "通用题", "突击题"], "cn"),
    ...[["pick", "卡片练习 · 按 JD 挑卡"], ["follow", "卡片练习 · 追问"], ["comment", "卡片练习 · 点评"], ["prep", "面试 · 准备"], ["compare", "面试 · 复盘对照"]].map(([k, l]) => mockPrompt(k, l, ["共用准则", "题库维护", "面试"], "cn")),
  ],
  picky_seeker_folder: (a) => { seekerMock.workspace.folder = String(a.folder).trim(); return view(); },
  picky_seeker_settings: (a) => { catalog().settings = structuredClone(a.settings as SeekerSettings); return view(); },
  picky_seeker_output_save: (a) => {
    const w = seekerMock.workspace;
    const lang = langOf(a);
    const draft = desk(lang).draft;
    // Pomelo's built-in template of the language (Rust output.rs `builtin`)
    if (!draft) throw new Error("还没有简历草稿");
    if (!String(a.directory).trim()) throw new Error("请选择导出到哪个文件夹");
    const filename = String(a.filename).trim();
    if (!/\.docx$/i.test(filename) || /[/\\:]/.test(filename)) throw new Error("文件名要以 .docx 结尾，不能有 / \\ :");
    const path = `${String(a.directory).trim()}/${filename}`;
    if (w.output.exports.some((e) => e.path === path)) throw new Error("已经有同名的文件了，请换个名字");
    // a record only: no copy of the résumé (Rust output.rs `Export`)
    const materials = [...new Set(draft.sections.flatMap((s) => s.entries).map((e) => e.source?.recordId).filter((id): id is string => !!id))];
    const item: SeekerExport = { id: crypto.randomUUID().replace(/-/g, ""), label: filename, path, lang, materials, createdAt: new Date().toISOString() };
    w.output.exports.unshift(item);
    if (a.jobId != null) associate(item.path, draft.jd, a.jobId as number);
    return { workspace: view(), export: item };
  },
  // a job's résumés: any file under cv/ by path (Rust output.rs `associate`, `job_resumes`)
  picky_seeker_job_resume: (a) => {
    const file = cvFiles().find((f) => f.path === a.path);
    if (!file) throw new Error("只能用 cv/ 里的简历");
    associate(file.path, "", a.jobId as number);
    return view();
  },
  picky_seeker_job_resumes: (a) => {
    const job = seekerMock.job(a.jobId as number);
    if (!job) throw new Error("岗位不存在，请重新选择");
    const files = cvFiles();
    const linked = seekerMock.workspace.output.links.filter((l) => l.job.id === job.id && l.job.createdAt === job.createdAt).flatMap((l) => { const f = files.find((x) => x.path === l.path); return f ? [{ path: l.path, name: f.name, lang: f.lang, current: l.current, jd: l.jd, job: l.job, createdAt: l.createdAt, exportId: f.exportId }] : []; });
    return { linked, files, noFolder: !seekerMock.workspace.folder, error: null };
  },
  picky_seeker_use_job: (a) => {
    const lang = langOf(a);
    const job = a.jobId == null ? null : seekerMock.job(a.jobId as number);
    if (a.jobId != null && !job) throw new Error("岗位不存在，请重新选择");
    const fresh = !desk(lang).draft;
    const draft = fresh ? initial(lang) : draftOf(lang)!;
    draft.job = job ? { id: job.id, createdAt: job.createdAt } : undefined;
    if (!job) delete draft.job;
    if (job?.note?.trim()) draft.jd = job.note.trim();
    save(draft); if (fresh) desk(lang).baseline = structuredClone(draft); return view();
  },
  picky_seeker_knowledge: () => {
    const folder = seekerMock.workspace.folder;
    if (!folder) return { path: null, library: null, companies: [], error: null };
    return { path: `${folder}/knowledge`, library: kbAdded ? "Seeker" : null, companies: shownCompanies(), error: null, cv: `${folder}/cv`, cvLibrary: cvAdded ? "Seeker" : null };
  },
  picky_seeker_company_names: (a) => {
    const next: Record<string, string> = {};
    for (const [folder, raw] of Object.entries(a.names as Record<string, string>)) {
      const name = raw.trim();
      if (!name || !COMPANIES.some((c) => c.folder === folder)) continue;
      if ([...name].length > 30) throw new Error(`「${name}」太长了，公司名称最多 30 个字`);
      if (Object.values(next).some((n) => nameKey(n) === nameKey(name))) throw new Error(`两家公司都叫「${name}」，请换一个名称`);
      next[folder] = name;
    }
    companyNames = next;
    return seekerHandlers.picky_seeker_knowledge({});
  },
  picky_seeker_knowledge_add: (a) => {
    const part = a.part === "cv" ? "cv" : "knowledge";
    if (!seekerMock.workspace.folder) throw new Error(`Seeker 文件夹里没有 ${part}/`);
    if (part === "cv" ? cvAdded : kbAdded) throw new Error(`${part}/ 已经在知识库里了`);
    if (part === "cv") cvAdded = true; else kbAdded = true;
    return seekerHandlers.picky_seeker_knowledge({});
  },
  picky_seeker_files: () => {
    const folder = seekerMock.workspace.folder;
    if (!folder) return { cv: [], material: [], transfer: false, error: null };
    const day = Math.floor(Date.now() / 1000);
    return {
      cv: cvFiles(),
      material: MATERIAL.map(([group, name], i) => ({ path: `${folder}/material/${group}/${name}`, name, group, size: 210000 + i * 5000, modified: day - i * 86400 })),
      transfer: true,
      error: null,
    };
  },
  picky_seeker_cv_text: (a) => {
    const name = String(a.path).split("/cv/").pop() ?? "";
    if (!CV.some((c) => c.name === name)) throw new Error("只能读 cv/ 里的简历");
    return cvText(name);
  },
  picky_seeker_job_company: (a) => jobCompany(a.jobId as number),
  picky_seeker_job_company_set: (a) => {
    const job = seekerMock.job(a.jobId as number);
    if (!job) throw new Error("岗位不存在，请重新选择");
    const folder = a.folder as string | null;
    if (folder && !COMPANIES.some((c) => c.folder === folder)) throw new Error("knowledge/ 里没有这家公司了");
    const w = seekerMock.workspace;
    const rest = (w.jobCompanies ?? []).filter((c) => !(c.jobId === job.id && c.jobCreatedAt === job.createdAt));
    w.jobCompanies = folder == null ? rest : [...rest, { jobId: job.id, jobCreatedAt: job.createdAt, folder }];
    return jobCompany(job.id);
  },
  picky_seeker_new: (a) => { const lang = langOf(a); const draft = initial(lang); save(draft); desk(lang).baseline = structuredClone(draft); return view(); },
  picky_seeker_save: (a) => save(a.draft as SeekerDraft),
  picky_seeker_delete_blocks: (a) => {
    const draft = draftOf(langOf(a));
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
    const freshEn = rec({ id: crypto.randomUUID(), lang: "en", title: "Pomelo Notes 2: Sample Project", role: "Sole Developer", time: "Aug 2026 – Present", points: pts("Built a sample feature"), source: "en-modules/5-PROJECTS.md#Pomelo Notes 2: Sample Project:1", revision: 0 });
    const rule = rec({ id: crypto.randomUUID(), kind: "rule", category: "素材润色", title: "有效个人约定", points: [{ id: "p1", text: "不单列业务分析：技能区不单列「业务分析」" }], source: "rules/cn-modules.md#有效个人约定:1", revision: 0 });
    const old = records.find((r) => r.id === "r-orange");
    const items: SeekerImport["items"] = [{ incoming: fresh, current: null }, { incoming: freshEn, current: null }, { incoming: rule, current: null }];
    if (old) items.push({ incoming: { ...structuredClone(old), source: old.source ?? "cn-modules/4 - 项目经历.md#橙子识别:1", points: pts("**模型迭代**：三轮调参，准确率 0.93", "**类别均衡**：重采样处理样本失衡") }, current: structuredClone(old) });
    const gone = records.filter((r) => r.source?.startsWith("rules/cn-modules.md#中文素材润色规则"));
    return { items: items.filter((i) => !records.some((r) => r.source === i.incoming.source && r.id !== i.incoming.id)), gone, skipped: ["TRANSFER.md：不是要导入的资料", "en-modules/0-Archived.md：归档的不导入", "rules/qbank.md：题库和面试的规则以后和 Rater 一起导入"] };
  },
  picky_seeker_import_confirm: (a) => {
    const prior = structuredClone(seekerMock.workspace);
    try {
      (a.records as SeekerRecord[]).forEach(put);
      for (const id of (a.remove as string[] | undefined) ?? []) seekerHandlers.picky_seeker_record_delete({ id });
    } catch (e) { seekerMock.workspace = prior; throw e; }
    return view();
  },
  picky_seeker_assemble: (a) => {
    const lang = langOf(a);
    const picks = a.selections as SeekerSelection[];
    const isSummary = (id: string) => catalog().records.some((r) => r.id === id && r.category === SUMMARY);
    if (picks.filter((p) => isSummary(p.recordId)).length > 1) throw new Error("Summary 只能选一条");
    const current = desk(lang).draft;
    const fresh = !!a.replace || !current;
    const draft = fresh ? initial(lang) : structuredClone(current!);
    if (fresh) { draft.jd = current?.jd ?? ""; if (current?.job) draft.job = current.job; if (lang === "en" && current?.form) draft.form = current.form; draft.sections.forEach((s) => { s.entries = s.entries.filter((e) => !picks.some((p) => p.recordId === e.source?.recordId)); }); }
    for (const pick of picks) {
      const r = catalog().records.find((r) => r.id === pick.recordId && materialIn(r, lang));
      if (!r) throw new Error("所选素材已删除或停用，请重新选材");
      if (pick.pointIds.some((id) => !choosable(r).some((p) => p.id === id))) throw new Error("素材的要点刚刚改过，请重新选择");
      if (draft.sections.some((s) => s.entries.some((e) => e.source?.recordId === r.id))) continue;
      const s = draft.sections.find((s) => s.category === r.category);
      if (s && r.category === SUMMARY) s.entries = [entry(r, pick.pointIds)];
      else if (s) s.entries.push(entry(r, pick.pointIds));
      else {
        const rank = (c: string) => (ORDER_EN.includes(c) ? ORDER_EN.indexOf(c) : ORDER_EN.length);
        const at = lang === "en" ? draft.sections.findIndex((x) => rank(x.category ?? "") > rank(r.category)) : -1;
        draft.sections.splice(at < 0 ? draft.sections.length : at, 0, { id: crypto.randomUUID(), category: r.category, title: title(r.category, lang), entries: [entry(r, pick.pointIds)] });
      }
    }
    save(draft); if (fresh) desk(lang).baseline = structuredClone(draft); return view();
  },
  picky_seeker_sources: (a): SourceChange[] => (desk(langOf(a)).draft?.sections.flatMap((s) => s.entries) ?? []).flatMap((e) => {
    const r = catalog().records.find((r) => r.id === e.source?.recordId);
    if (!r || !e.source || r.revision === e.source.revision) return [];
    return [{ entryId: e.id, title: r.title, before: structuredClone(e), after: entry(r, e.source.pointIds.filter((id) => choosable(r).some((p) => p.id === id)), e.id) }];
  }),
  picky_seeker_source_apply: (a) => {
    const change = a.change as SourceChange, draft = draftOf(langOf(a));
    const section = draft?.sections.find((s) => s.entries.some((e) => e.id === change.entryId));
    const index = section?.entries.findIndex((e) => e.id === change.entryId) ?? -1;
    if (!draft || !section || index < 0 || JSON.stringify(section.entries[index]) !== JSON.stringify(change.before)) throw new Error("草稿刚刚改过，请重新看来源差异");
    section.entries[index] = structuredClone(change.after); return save(draft);
  },
  picky_seeker_reorder: (a) => {
    const draft = draftOf(langOf(a)); if (!draft) throw new Error("还没有草稿");
    const ids = a.ids as string[];
    const rows = a.kind === "header" ? draft.header : a.kind === "section" ? draft.sections : a.kind === "entry" ? draft.sections.find((s) => s.id === a.parent)?.entries : a.kind === "point" ? draft.sections.flatMap((s) => s.entries).find((e) => e.id === a.parent)?.points : undefined;
    if (!rows || ids.length !== rows.length || new Set(ids).size !== ids.length || rows.some((r) => !ids.includes(r.id))) throw new Error("内容刚刚变了，请重新排序");
    rows.sort((l, r) => ids.indexOf(l.id) - ids.indexOf(r.id)); return save(draft);
  },
  picky_seeker_apply: (a) => { const before = a.before as SeekerDraft; if (JSON.stringify(desk(langOfDraft(before)).draft) !== JSON.stringify(before)) throw new Error("草稿刚刚改过，请重新生成建议"); return save(a.after as SeekerDraft); },
  picky_seeker_suggest: (a) => {
    const c = catalog();
    const record = a.kind === "material" ? structuredClone(c.records.find((r) => r.id === a.recordId) ?? null) : null;
    const lang: SeekerLang = record ? (record.lang === "en" ? "en" : "cn") : langOf(a);
    const model = a.kind === "select" ? c.settings.selectionModel : c.settings.reviewModel;
    if (!model) throw new Error("请为这项工作选择模型");
    if (model.startsWith("cloud:") && !c.settings.cloudAllowed) throw new Error("Seeker 资料还没允许发给云端");
    const own = desk(lang).draft;
    const draft = ["review", "polish", "summary"].includes(a.kind as string) ? structuredClone(own) : null;
    if (["review", "polish", "summary"].includes(a.kind as string) && !draft) throw new Error("还没有简历草稿");
    const base = { kind: a.kind as string, model: "模拟模型", note: "", advice: "", selections: [] as SeekerSelection[], beforeDraft: draft, afterDraft: null as SeekerDraft | null, beforeRecord: record, afterRecord: null as SeekerRecord | null };
    const ref = own?.job;
    const job = ref ? seekerMock.job(ref.id) : undefined;
    const folder = job && (seekerMock.workspace.jobCompanies?.find((c) => c.jobId === job.id && c.jobCreatedAt === job.createdAt)?.folder ?? matched(job.company));
    if (["select", "review", "summary"].includes(a.kind as string) && job) base.note = !folder ? "这个岗位没有对上 knowledge/ 里的公司" : !kbAdded ? "knowledge/ 还没加进知识库" : `参考了${companyList().find((c) => c.folder === folder)?.name}的 3 段公司资料`;
    if (a.kind === "select") {
      if (!own?.jd.trim()) throw new Error("请先填写目标 JD");
      const mine = c.records.filter((r) => materialIn(r, lang));
      return { ...base, advice: lang === "en" ? "（模拟）JD 偏数据分析：Agent Summary、两个项目和实习。" : "（模拟）JD 偏数据分析：选两个项目和实习。", selections: [...mine.filter((r) => r.category === SUMMARY).slice(1, 2), ...mine.filter((r) => ["项目", "工作"].includes(r.category)).slice(0, 3)].map((r) => ({ recordId: r.id, pointIds: choosable(r).filter((p) => !isStack(p.text)).slice(0, 3).map((p) => p.id) })) };
    }
    if (a.kind === "review") return { ...base, advice: lang === "en" ? "需处理\n- 「Pomelo Notes」→ 4 条超过 Resume 每段 3 条 → 删一条\n\n可选优化\n- 「Built」重复两次 → 换一个更具体的动词" : "需处理\n- 「柚子笔记」第 2 条 → 缺少结果 → 补一个可核实的效果\n\n可选优化\n- 工作经历 → 动词重复 → 换一个更具体的动作" };
    if (a.kind === "summary") {
      if (lang !== "en") throw new Error("只有英文简历有 Summary");
      if (!draft!.jd.trim()) throw new Error("请先填写目标 JD");
      const after = structuredClone(draft!);
      const s = after.sections.find((x) => x.category === SUMMARY)?.entries[0];
      if (!s) throw new Error("草稿里还没有 Summary");
      s.note = `${s.note} (Simulated: tuned for the JD's data analysis focus.)`;
      return { ...base, advice: "（模拟）按 JD 加了数据分析的方向。", afterDraft: after };
    }
    if (draft) { const after = structuredClone(draft); const p = after.sections.flatMap((s) => s.entries).flatMap((e) => e.points)[0]; if (p) p.text = `${p.text}（模拟润色）`; return { ...base, advice: "（模拟）只改了第一条。", afterDraft: after }; }
    if (record) return { ...base, advice: "（模拟）概览更具体。", afterRecord: { ...record, text: `${record.text}（模拟润色）` } };
    throw new Error("素材已不存在");
  },
};
