// One-off: the user's Markdown todo list (「近期待办」) → Pomelo data file with the `todo` module
// (default prototype/todo-import.json, gitignored). Import it in the app: 设置 → 导入.
// Usage: pnpm import:todo [input.md] [output.json]
// Mapping (agreed in the round-2 questionnaire):
// - `### heading` → group (emoji kept); `- [ ]` / `- [x]` → task (indent = subtask); `<!-- {"fold":true} -->` → collapsed
// - in the Pick Up group, GOAL_TITLES become goals; other items stay tasks
// - plain bullets under a task → its note (JY Gift's gift ideas); a top-level plain bullet → an undone task
//   (「每周六交周报」; turned into a 重复 task by hand in the app)
// - any line mentioning 密码 is dropped, never written out
// - dates in titles: 「1/22–1/25」 or 「1 月 27–29 日」 → time range; 「10 月 14 日」 → due date (months before the
//   current one roll into next year)
import { readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const input = process.argv[2] ?? resolve(homedir(), "Downloads/近期待办.md");
const output = process.argv[3] ?? resolve(root, "prototype/todo-import.json");
const GOAL_TITLES = ["Personal Website", "Personal VPN", "Pub2MD Agent", "Seeker"];

type Task = {
  title: string;
  note?: string;
  startDate?: string;
  dueDate?: string;
  done?: boolean;
  collapsed?: boolean;
  children: Task[];
};
type Group = { name: string; goals: { name: string; note?: string; tasks: Task[] }[]; tasks: Task[] };

const now = new Date();
const ymd = (m: number, d: number) => {
  const y = m < now.getMonth() + 1 ? now.getFullYear() + 1 : now.getFullYear();
  return `${y}-${String(m).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
};

function dates(title: string): Pick<Task, "startDate" | "dueDate"> {
  const plain = title.replace(/\*\*/g, "");
  let m = plain.match(/(\d{1,2})\/(\d{1,2})\s*[–—-]\s*(\d{1,2})\/(\d{1,2})/);
  if (m) return { startDate: ymd(+m[1], +m[2]), dueDate: ymd(+m[3], +m[4]) };
  m = plain.match(/(\d{1,2})\s*月\s*(\d{1,2})\s*[–—-]\s*(\d{1,2})\s*日/);
  if (m) return { startDate: ymd(+m[1], +m[2]), dueDate: ymd(+m[1], +m[3]) };
  m = plain.match(/(\d{1,2})\s*月\s*(\d{1,2})\s*日/);
  if (m) return { dueDate: ymd(+m[1], +m[2]) };
  return {};
}

const groups: Group[] = [];
let group: Group | null = null;
const stack: { indent: number; task: Task }[] = []; // open task chain for nesting
let noteTarget: Task | null = null; // where continuation lines go
let skipping = false;
let skipped = 0;

const addNote = (t: Task, line: string) => {
  t.note = t.note ? `${t.note}\n${line}` : line;
};

for (const raw of readFileSync(input, "utf8").split(/\r?\n/)) {
  if (!raw.trim()) continue;
  const heading = raw.match(/^(#{1,6})\s+(.*)$/);
  if (heading) {
    skipping = false;
    if (heading[1].length >= 2) {
      group = { name: heading[2].trim(), goals: [], tasks: [] };
      groups.push(group);
      stack.length = 0;
      noteTarget = null;
    }
    continue;
  }
  if (!group) continue;
  const indent = raw.match(/^\s*/)![0].length;
  const box = raw.match(/^\s*[-*+]\s+\[([ xX])\]\s+(.*)$/);
  const bullet = !box && raw.match(/^\s*[-*+]\s+(.*)$/);
  const text = (box ? box[2] : bullet ? bullet[1] : raw.trim()).trim();

  if (/密码/.test(text)) {
    skipping = true;
    skipped++;
    continue;
  }
  if (!box && !bullet) {
    if (!skipping && noteTarget) addNote(noteTarget, text); // continuation line
    continue;
  }
  skipping = false;
  while (stack.length && stack[stack.length - 1].indent >= indent) stack.pop();
  const parent = stack[stack.length - 1]?.task ?? null;

  if (bullet && parent) {
    addNote(parent, `- ${text}`); // gift ideas etc.
    noteTarget = parent;
    continue;
  }
  let title = text;
  let note: string | undefined;
  const collapsed = /<!--\s*\{\s*"fold"\s*:\s*true\s*\}\s*-->/.test(title);
  title = title.replace(/<!--.*?-->/g, "").trim();
  if (bullet) {
    const i = title.indexOf("。");
    if (i > 0) [title, note] = [title.slice(0, i), title.slice(i + 1).trim() || undefined];
  }
  const done = box ? box[1].toLowerCase() === "x" : false;
  const task: Task = { title, ...(note ? { note } : {}), ...(done ? { done } : dates(title)), ...(collapsed ? { collapsed } : {}), children: [] };
  (parent ? parent.children : group.tasks).push(task);
  stack.push({ indent, task });
  noteTarget = task;
}

// Pick Up projects → goals (their subtasks become the goal's tasks, the note stays on the goal).
for (const g of groups) {
  if (!/pick up/i.test(g.name)) continue;
  g.goals = g.tasks.filter((t) => GOAL_TITLES.includes(t.title)).map((t) => ({ name: t.title, ...(t.note ? { note: t.note } : {}), tasks: t.children }));
  g.tasks = g.tasks.filter((t) => !GOAL_TITLES.includes(t.title));
}

const strip = (ts: Task[]): object[] => ts.map(({ children, ...t }) => ({ ...t, ...(children.length ? { children: strip(children) } : {}) }));
const todo = { groups: groups.map((g) => ({ name: g.name, goals: g.goals.map((x) => ({ ...x, tasks: strip(x.tasks) })), tasks: strip(g.tasks) })) };
writeFileSync(
  output,
  JSON.stringify({ format: "pomelo-export", formatVersion: 2, exportedAt: now.toISOString(), source: "近期待办.md", modules: { todo } }, null, 2) + "\n",
);

const all: Task[] = [];
const walk = (ts: Task[]) => ts.forEach((t) => (all.push(t), walk(t.children)));
groups.forEach((g) => (walk(g.tasks), g.goals.forEach((x) => walk(x.tasks))));
console.log(`已生成 ${output}`);
console.log(`${groups.length} 个分组：${groups.map((g) => g.name).join("、")}`);
console.log(`${groups.reduce((n, g) => n + g.goals.length, 0)} 个目标，${all.length} 个任务（已完成 ${all.filter((t) => t.done).length}，带日期 ${all.filter((t) => t.dueDate).length}）`);
if (skipped) console.log(`跳过 ${skipped} 行含「密码」的内容，请移到「密码」App`);
