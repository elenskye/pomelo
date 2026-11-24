// 写字的框的测试台（src/dev/edlab.ts；docs/testing.md「写字的框」）。自己起一个 Vite（不占 1420），打开 `?mock&edlab`：
//   node scripts/editor-lab.mjs            无头 Chrome（pnpm check 跑这个）
//   node scripts/editor-lab.mjs --safari   Safari，也就是 WebKit（窗口是 WebKit，改了写字的框、它的样式就跑一次）
// 有一项不过就列出来、退出码 1。Chrome 用 visual-atelier 那边的 playwright-core（和 ui-lint 一样）。
import { createRequire } from "node:module";
import { createServer as httpServer } from "node:http";
import { execFileSync } from "node:child_process";
import { homedir } from "node:os";
import { createServer } from "vite";

const safari = process.argv.includes("--safari");
const root = new URL("..", import.meta.url).pathname;
const vite = await createServer({ root, logLevel: "error", server: { port: 1437, strictPort: false, hmr: false } });
await vite.listen();
const port = vite.config.server.port ?? 1437;
const url = (q) => `http://localhost:${port}/?mock&edlab=${q}`;

const report = (r) => {
  const n = Object.values(r.checks).reduce((a, b) => a + b, 0);
  console.log(`editor-lab（${safari ? "Safari" : "Chrome"}）：${r.states} 个样子，${n} 项检查（${Object.entries(r.checks).map(([k, v]) => `${k} ${v}`).join("、")}），${r.ms} 毫秒`);
  if (!r.fails.length) return true;
  console.error(`没过（${r.fails.length}）：\n${r.fails.map((f) => `- [${f.check}] ${f.where}：${f.detail}`).join("\n")}`);
  return false;
};

let ok = false;
try {
  if (!safari) {
    const require = createRequire(`${process.env.VA_DIR ?? `${homedir()}/Documents/GitHub/visual-atelier`}/package.json`);
    const { chromium } = require("playwright-core");
    const browser = await chromium.launch({ channel: "chrome", args: ["--hide-scrollbars"] });
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    const errors = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto(url(1));
    await page.waitForFunction(() => window.__edlab?.done, null, { timeout: 120_000 }).catch(() => {});
    const r = await page.evaluate(() => window.__edlab ?? null);
    await browser.close();
    if (!r) throw new Error(`测试台没跑完${errors.length ? `：${errors[0]}` : ""}`);
    ok = report(r);
  } else {
    // Safari posts the result back; the tab stays open (no scripting Safari)
    const got = new Promise((resolve) => {
      const srv = httpServer((req, res) => {
        let body = "";
        req.on("data", (d) => (body += d));
        req.on("end", () => {
          res.end("ok");
          srv.close();
          resolve(JSON.parse(body));
        });
      }).listen(0, "127.0.0.1", () => execFileSync("open", ["-g", "-a", "Safari", url(srv.address().port)]));
    });
    const r = await Promise.race([got, new Promise((_, no) => setTimeout(() => no(new Error("Safari 两分钟没交结果（后台标签页被挂起了？把 Safari 拿到前面再跑一次）")), 120_000))]);
    console.log(`（${r.agent}）`);
    ok = report(r);
  }
} catch (e) {
  console.error(String(e?.message ?? e));
} finally {
  await vite.close();
}
process.exit(ok ? 0 : 1);
