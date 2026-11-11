// src/dev/atlasMock.json: the made-up 「呈现结果」 the browser mock (?mock#/atlas) shows — visual-atelier's sample
// year (schemes/pomelo/atlas/tabloid/sample.mjs), in three runs: 2026 written up to today, 2025 twice.
// Run when the sample or the data shape changes:  node scripts/atlas-mock.mjs
import { writeFileSync } from "node:fs";
import path from "node:path";
import os from "node:os";
import { pathToFileURL } from "node:url";

const va = process.env.VISUAL_ATELIER || path.join(os.homedir(), "Documents/GitHub/visual-atelier");
const { sampleYear } = await import(pathToFileURL(path.join(va, "schemes/pomelo/atlas/tabloid/sample.mjs")).href);
const shift = (run, year) => JSON.parse(JSON.stringify(run).replaceAll("2025", "#LAST#").replaceAll("2026", String(year)).replaceAll("#LAST#", String(year - 1)));
const runs = [
  { ...sampleYear({ partial: true }), id: 3 },
  { ...shift(sampleYear({ other: true, skip: ["jobs"] }), 2025), id: 2, createdAt: "2026-03-02T12:00:00.000Z" },
  { ...shift(sampleYear({ skip: ["drawing", "money"] }), 2025), id: 1, createdAt: "2025-12-31T13:20:00.000Z" },
];
writeFileSync("src/dev/atlasMock.json", JSON.stringify(runs));
console.log("src/dev/atlasMock.json", runs.length, "份");
