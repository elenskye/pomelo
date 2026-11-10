// 在真的 WebKit 里遥控页面（docs/testing.md「在 Safari 里遥控」）：只在 `pnpm dev` 的 `?mock&drive=<端口>` 里跑。
// 有些毛病只在窗口的 WebKit 里出现（被「把焦点滚进视野」滚上去的盒子、合成层没跟着变的裁切），内置浏览器复现不了。
// 页面每隔一会儿向本机的 `node scripts/drive.mjs <端口> <脚本.js>` 要一段脚本，当成 async 函数体跑完，把 return 的
// 东西 POST 回去。脚本里有 `sleep(ms)`、`$`（querySelector）、`$$`（querySelectorAll 的数组）、`type(input, 字)`
// （照 React 认的办法填输入框）、`key(键, 选项)`（在焦点上发 keydown）。
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function type(el: HTMLInputElement | HTMLTextAreaElement, text: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, text);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

function key(k: string, init: KeyboardEventInit = {}) {
  (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init }));
}

export async function runDrive(port: string) {
  const base = `http://127.0.0.1:${port}`;
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor as new (...args: string[]) => (...args: unknown[]) => Promise<unknown>;
  const $ = (s: string) => document.querySelector(s);
  const $$ = (s: string) => [...document.querySelectorAll(s)];
  for (;;) {
    let src = "";
    try {
      const r = await fetch(`${base}/next`);
      if (r.status === 200) src = await r.text();
    } catch {
      // nobody is listening yet
    }
    if (!src) {
      await sleep(500);
      continue;
    }
    let out: unknown;
    try {
      out = await new AsyncFunction("sleep", "$", "$$", "type", "key", src)(sleep, $, $$, type, key);
    } catch (e) {
      out = { error: String(e), stack: e instanceof Error ? e.stack : undefined };
    }
    try {
      await fetch(`${base}/done`, { method: "POST", body: JSON.stringify(out ?? null) });
    } catch {
      // the listener left
    }
  }
}
