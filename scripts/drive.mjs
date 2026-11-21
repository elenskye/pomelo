// 在 Safari 里遥控 `?mock&drive=<端口>` 的页面（docs/testing.md「在 Safari 里遥控」）：
//   node scripts/drive.mjs <端口> <脚本.js> [最多等几秒，默认 30]
// 把脚本交给页面（src/dev/drive.ts）跑一次，打出它 return 的东西（JSON），退出。只听本机。
import { createServer } from "node:http";
import { readFileSync } from "node:fs";

const [port = "1498", file, wait = "30"] = process.argv.slice(2);
if (!file) {
  console.error("用法：node scripts/drive.mjs <端口> <脚本.js> [最多等几秒]");
  process.exit(2);
}
const script = readFileSync(file, "utf8");
let given = false;
const cors = { "Access-Control-Allow-Origin": "*", "Access-Control-Allow-Headers": "*", "Cache-Control": "no-store" };
createServer((req, res) => {
  if (req.method === "OPTIONS") return void res.writeHead(204, cors).end();
  if (req.method === "GET" && req.url === "/next") {
    if (given) return void res.writeHead(204, cors).end();
    given = true;
    return void res.writeHead(200, { ...cors, "Content-Type": "text/plain; charset=utf-8" }).end(script);
  }
  if (req.method === "POST" && req.url === "/done") {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      res.writeHead(200, cors).end("ok");
      try {
        console.log(JSON.stringify(JSON.parse(body), null, 1));
      } catch {
        console.log(body);
      }
      process.exit(0);
    });
    return;
  }
  res.writeHead(404, cors).end();
}).listen(Number(port), "127.0.0.1");
setTimeout(() => {
  console.error(given ? "页面拿了脚本，没等到结果" : "没有页面来拿脚本（Safari 里开着 ?mock&drive=<端口> 吗）");
  process.exit(1);
}, Number(wait) * 1000);
