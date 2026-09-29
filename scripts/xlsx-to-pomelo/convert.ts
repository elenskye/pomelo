// One-off: prototype/柚儿园.xlsx → Pomelo export JSON (default prototype/pomelo-import.json, gitignored).
// Usage: pnpm import:xlsx [input.xlsx] [output.json]   (Node >= 23 runs .ts via type stripping)
// Add one converter per module as its phase lands; the app skips modules it doesn't know yet.
import ExcelJS from "exceljs";
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";

type Status = "unmarked" | "todo" | "doing" | "done";
type Row = { getCell(i: number): { text: string; value: unknown } };
type Sheet = { rowCount: number; getRow(i: number): Row };

const root = resolve(import.meta.dirname, "../..");
const input = process.argv[2] ?? resolve(root, "prototype/柚儿园.xlsx");
const output = process.argv[3] ?? resolve(root, "prototype/pomelo-import.json");

const cell = (row: Row, i: number): string | null => row.getCell(i).text.trim() || null;
const stars = (s: string | null) => (s ? [...s].filter((c) => c === "★").length || null : null);

function expectHeaders(ws: Sheet, rowNo: number, headers: string[], sheet: string) {
  const got = headers.map((_, i) => cell(ws.getRow(rowNo), i + 1));
  if (got.join("|") !== headers.join("|")) {
    throw new Error(`${sheet}的表头变了：期望 ${headers.join("、")}，实际 ${got.join("、")}`);
  }
}

// Books & films sheets share one layout: 名称, 作者/导演, 国家, 状态, 尝尝咸淡, 备注 (data from row 4).
function media(ws: Sheet, sheetName: string, headers: string[], status: Record<string, Status>) {
  expectHeaders(ws, 3, headers, sheetName);
  const out = [];
  for (let r = 4; r <= ws.rowCount; r++) {
    const row = ws.getRow(r);
    const title = cell(row, 1);
    if (!title) continue;
    const st = cell(row, 4);
    if (st && !(st in status)) throw new Error(`${sheetName}第 ${r} 行：不认识的状态「${st}」`);
    out.push({
      title,
      creator: cell(row, 2)?.replace(/\s*[&＆]\s*/g, " / ") ?? null, // unify separators to " / "
      country: cell(row, 3),
      status: st ? status[st] : ("unmarked" as Status),
      rating: stars(cell(row, 5)),
      finishedMonth: null,
      note: cell(row, 6),
    });
  }
  return out;
}

const books = (ws: Sheet) =>
  media(ws, "摸鱼读读", ["书名", "作者", "国家", "状态", "尝尝咸淡", "备注"], { 想读: "todo", 在读: "doing", 已读: "done" }).map(
    ({ creator, ...r }) => ({ ...r, author: creator, kind: "虚构" }), // 类型 not in sheet: default 虚构 (kind = name)
  );

const films = (ws: Sheet) =>
  media(ws, "好奇看看", ["名称", "作者/导演", "国家", "状态", "尝尝咸淡", "备注"], { 想看: "todo", 在看: "doing", 已看: "done" }).map(
    ({ creator, ...r }) => ({ ...r, director: creator, kind: "电影" }), // sheet is all movies
  );

// 随意画画: 课程, 编号, 类型, 课名, 状态, 完成日期, 备注 → nested courses (first-appearance order) → lessons (row order).
function drawing(ws: Sheet) {
  expectHeaders(ws, 3, ["课程", "编号", "类型", "课名", "状态", "完成日期", "备注"], "随意画画");
  const status: Record<string, Status> = { 未开始: "todo", 进行中: "doing", 已完成: "done" };
  const courses = new Map<string, { name: string; lessons: object[] }>();
  for (let r = 4; r <= ws.rowCount; r++) {
    const row = ws.getRow(r);
    const name = cell(row, 1);
    const title = cell(row, 4);
    if (!name || !title) continue;
    const st = cell(row, 5);
    if (st && !(st in status)) throw new Error(`随意画画第 ${r} 行：不认识的状态「${st}」`);
    const code = cell(row, 2);
    const date = row.getCell(6).value;
    if (!courses.has(name)) courses.set(name, { name, lessons: [] });
    courses.get(name)!.lessons.push({
      code: code === "DLC" || code === "LIVE" ? null : code, // those "codes" duplicate 类型
      kind: cell(row, 3),
      title,
      status: st ? status[st] : "unmarked",
      finishedMonth: date instanceof Date ? date.toISOString().slice(0, 7) : null,
      note: cell(row, 7),
    });
  }
  return [...courses.values()];
}

// 出门走走: 场景, 类别, 物品, 本次打包, 备注 (header row 4) → templates (first-appearance order). No trips:
// the ✓ column is per-trip state, empty in the prototype.
function packing(ws: Sheet) {
  expectHeaders(ws, 4, ["场景", "类别", "物品", "本次打包", "备注"], "出门走走");
  const templates = new Map<string, { name: string; items: object[] }>();
  for (let r = 5; r <= ws.rowCount; r++) {
    const row = ws.getRow(r);
    const name = cell(row, 1);
    const item = cell(row, 3);
    if (!name || !item) continue;
    if (!templates.has(name)) templates.set(name, { name, items: [] });
    templates.get(name)!.items.push({ category: cell(row, 2), name: item, note: cell(row, 5) });
  }
  return { templates: [...templates.values()], trips: [] };
}

const wb = new ExcelJS.Workbook();
await wb.xlsx.readFile(input);
const sheet = (name: string): Sheet => {
  const ws = wb.getWorksheet(name);
  if (!ws) throw new Error(`找不到工作表「${name}」`);
  return ws as unknown as Sheet;
};

const modules = { books: books(sheet("摸鱼读读")), films: films(sheet("好奇看看")), drawing: drawing(sheet("随意画画")), packing: packing(sheet("出门走走")) };
writeFileSync(
  output,
  JSON.stringify({ format: "pomelo-export", formatVersion: 1, exportedAt: new Date().toISOString(), source: "柚儿园.xlsx", modules }, null, 2) + "\n",
);

const count = (xs: { status: Status }[], s: Status) => xs.filter((x) => x.status === s).length;
const summary = (label: string, unit: string, verbs: [string, string, string], xs: { status: Status }[]) =>
  `${label} ${xs.length} ${unit}：${verbs[2]} ${count(xs, "done")} · ${verbs[1]} ${count(xs, "doing")} · ${verbs[0]} ${count(xs, "todo")} · 待标记 ${count(xs, "unmarked")}`;
console.log(`已生成 ${output}`);
console.log(summary("摸鱼读读", "本", ["想读", "在读", "已读"], modules.books));
console.log(summary("好奇看看", "部", ["想看", "在看", "已看"], modules.films));
const lessons = modules.drawing.flatMap((c) => c.lessons as { status: Status }[]);
console.log(`${summary("随意画画", "节", ["未开始", "进行中", "已完成"], lessons)}（${modules.drawing.length} 门课）`);
const tpls = modules.packing.templates;
console.log(`出门走走 ${tpls.length} 个场景模板，共 ${tpls.reduce((n, t) => n + t.items.length, 0)} 件物品`);
