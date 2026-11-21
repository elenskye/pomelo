// 流畅度自测的收件人（docs/testing.md「流畅度」）：node scripts/perf-listen.mjs <端口> <结果.json>
// 收到 src/dev/perf.ts 发来的一份结果就写文件、打一张表、退出。只听本机。
import { createServer } from "node:http";
import { writeFileSync } from "node:fs";

const [port = "1499", file] = process.argv.slice(2);
createServer((req, res) => {
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", () => {
    res.end("ok");
    if (req.method !== "POST") return;
    const r = JSON.parse(body);
    if (file) writeFileSync(file, JSON.stringify(r, null, 1));
    console.log(`${r.w}×${r.h} @${r.dpr}  手画的元素 ${r.hands}  ${r.css ? `样式：${r.css}` : ""}`);
    console.log("首帧  最长  在第  卡掉  帧数  手画  做什么");
    for (const s of r.out) console.log(`${String(s.first).padStart(4)}  ${String(s.worst).padStart(4)}  ${String(s.at ?? 0).padStart(4)}  ${String(s.jank).padStart(4)}  ${String(s.frames).padStart(4)}  ${String(s.hand ?? 0).padStart(4)}  ${s.what}`);
    const sum = (k) => r.out.reduce((a, s) => a + s[k], 0);
    console.log(`合计：卡掉 ${sum("jank")} ms，最长的一帧 ${Math.max(...r.out.map((s) => s.worst))} ms`);
    process.exit(0);
  });
}).listen(Number(port), "127.0.0.1");
