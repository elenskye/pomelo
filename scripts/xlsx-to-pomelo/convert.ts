// One-off: prototype/柚儿园.xlsx → Pomelo export JSON (default prototype/pomelo-import.json, gitignored).
// Usage: pnpm import:xlsx [input.xlsx] [output.json]   (Node >= 23 runs .ts via type stripping)
// Add one converter per module as its phase lands; the app skips modules it doesn't know yet.
import ExcelJS from "exceljs";
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";

type Status = "unmarked" | "todo" | "doing" | "done";
type Row = { getCell(i: number): { text: string } };
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

function books(ws: Sheet) {
  expectHeaders(ws, 3, ["书名", "作者", "国家", "状态", "尝尝咸淡", "备注"], "摸鱼读读");
  const status: Record<string, Status> = { 想读: "todo", 在读: "doing", 已读: "done" };
  const out = [];
  for (let r = 4; r <= ws.rowCount; r++) {
    const row = ws.getRow(r);
    const title = cell(row, 1);
    if (!title) continue;
    const st = cell(row, 4);
    if (st && !(st in status)) throw new Error(`摸鱼读读第 ${r} 行：不认识的状态「${st}」`);
    out.push({
      title,
      author: cell(row, 2),
      country: cell(row, 3),
      status: st ? status[st] : "unmarked",
      rating: stars(cell(row, 5)),
      finishedMonth: null,
      note: cell(row, 6),
    });
  }
  return out;
}

const wb = new ExcelJS.Workbook();
await wb.xlsx.readFile(input);
const sheet = (name: string): Sheet => {
  const ws = wb.getWorksheet(name);
  if (!ws) throw new Error(`找不到工作表「${name}」`);
  return ws as unknown as Sheet;
};

const modules = { books: books(sheet("摸鱼读读")) };
writeFileSync(
  output,
  JSON.stringify({ format: "pomelo-export", formatVersion: 1, exportedAt: new Date().toISOString(), source: "柚儿园.xlsx", modules }, null, 2) + "\n",
);

const count = (xs: { status: Status }[], s: Status) => xs.filter((x) => x.status === s).length;
const b = modules.books;
console.log(`已生成 ${output}`);
console.log(
  `摸鱼读读 ${b.length} 本：已读 ${count(b, "done")} · 在读 ${count(b, "doing")} · 想读 ${count(b, "todo")} · 待标记 ${count(b, "unmarked")}`,
);
