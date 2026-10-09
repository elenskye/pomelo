// One-off: the applications recorded in Seeker's README (「### 投递」 table) → Pomelo data file with the
// `jobs` module (default prototype/jobs-import.json, gitignored). Import it in the app: 设置 → 导入
// (replaces only 投石问路; the file has no status list, so the app's current statuses are kept).
// Usage: pnpm import:seeker [Seeker dir] [output.json]
// Mapping (steps use the app's default status names): 结果 「已拒」 → 已投递 ✓ … 被拒 ✓ · 「已录取」 → every
// step ✓ · 「已提交」 → 已投递 ✓; DETAILS.skip drops steps a target never had (BOCSG: no 网申 / 笔试). The first
// backticked file in 投出的材料 → 简历 (found under cv/). Company / 岗位 / 地点 / 渠道 come from DETAILS,
// copied from each target's knowledge/seek-<目标>/seek-<目标>.md (prose, not worth parsing).
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const seeker = process.argv[2] ?? resolve(homedir(), "Documents/Seeker");
const output = process.argv[3] ?? resolve(root, "prototype/jobs-import.json");

const STEPS = ["已投递", "网申测评 / 笔试", "面试", "Offer", "已接受"];
const DETAILS: Record<string, { company: string; position: string; location: string; channel: string; on?: Record<string, string>; skip?: string[]; note?: string }> = {
  KeyReply: { company: "KeyReply", position: "软件开发实习", location: "新加坡", channel: "学校", note: "CV 阶段被拒（GDipSA 提案渠道）" },
  BOC: { company: "中国银行新加坡分行", position: "Application Development Assistant（实习）", location: "新加坡", channel: "学校", on: { 面试: "2026-08-13" }, skip: ["网申测评 / 笔试"], note: "2026-09-07 入职，金融科技部" },
  "NUS-ISS 学院代投": { company: "NUS-ISS 学院代投", position: "实习（学院转投多家企业）", location: "新加坡", channel: "学校" },
};

const readme = readFileSync(resolve(seeker, "README.md"), "utf8");
const section = readme.split(/^### 投递\s*$/m)[1]?.split(/^#{2,3} /m)[0];
if (!section) throw new Error("README 里找不到「### 投递」");
const rows = section
  .split("\n")
  .filter((l) => l.startsWith("|") && !/^\|\s*-/.test(l) && !l.includes("投出的材料"))
  .map((l) => l.split("|").slice(1, -1).map((c) => c.trim()));

const home = homedir();
const findCv = (file: string) => {
  for (const dir of ["cv/en", "cv/cn", "cv"]) if (existsSync(resolve(seeker, dir, file))) return resolve(seeker, dir, file);
  return null;
};
const tilde = (p: string) => (p.startsWith(`${home}/`) ? `~/${p.slice(home.length + 1)}` : p);

const jobs = rows.map(([target, material, result]) => {
  const d = DETAILS[target];
  if (!d) throw new Error(`没有「${target}」的岗位信息，请补到 DETAILS`);
  const reached = result.includes("已录取") ? "已接受" : "已投递";
  const steps = STEPS.filter((n) => !d.skip?.includes(n)).map((status) => ({ status, done: STEPS.indexOf(status) <= STEPS.indexOf(reached), on: d.on?.[status] }));
  if (result.includes("已拒")) steps.push({ status: "被拒", done: true, on: undefined });
  const file = /`([^`]+)`/.exec(material)?.[1];
  const cv = file ? findCv(file) : null;
  return {
    company: d.company,
    position: d.position,
    location: d.location,
    channel: d.channel,
    resume: cv ? tilde(cv) : null,
    steps,
    note: d.note ?? null,
  };
});

writeFileSync(
  output,
  JSON.stringify({ format: "pomelo-export", formatVersion: 9, exportedAt: new Date().toISOString(), source: "Seeker/README.md", modules: { jobs: { jobs } } }, null, 2) + "\n",
);
console.log(`${jobs.length} 条投递 → ${output}`);
for (const j of jobs) console.log(`  ${j.company} · ${j.steps.filter((s) => s.done).pop()?.status}${j.resume ? "" : "（没找到简历文件）"}`);
