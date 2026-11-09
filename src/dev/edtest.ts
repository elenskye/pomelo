// 写字的框在 WebKit 里的自测（docs/testing.md「写字的框」）：只在 `pnpm dev` 的 `?mock&edtest=<端口>` 里跑。
// 字里的柚儿园图标（`:pm-名字:`，shared/editor/inlineIcon.ts）在框里是一个光标进不去的小块；光标在它前后、在它后面
// 打字、退格把它整个删掉，各家浏览器做法不一样，Pomelo 的窗口是 WebKit，所以在 Safari 里用真的编辑命令走一遍，
// 结果 POST 给本机的收件人（`node scripts/perf-listen.mjs` 不认这份；用 `nc -l` 或随手起一个收 POST 的就行）。
import { select, selectionIn, serialize } from "@/shared/editor/editable";
import { liteFlavor } from "@/shared/editor/lite";
import { markdownFlavor } from "@/shared/editor/markdown";
import "@/shared/editor/MdMenu"; // hands the icon drawings to the editors

type Row = { what: string; ok: boolean; got: string };

export async function runEdTest(port: string) {
  await new Promise((r) => setTimeout(r, 600));
  const rows: Row[] = [];
  const check = (what: string, got: unknown, want: unknown) => rows.push({ what, ok: JSON.stringify(got) === JSON.stringify(want), got: JSON.stringify(got) });
  for (const [name, flavor] of [["lite", liteFlavor({ tags: true })], ["markdown", markdownFlavor()]] as const) {
    const el = document.createElement("div");
    el.setAttribute("contenteditable", "plaintext-only");
    el.style.cssText = "position:fixed;left:20px;top:20px;width:400px;z-index:99999;background:#fff;color:#000;white-space:pre-wrap;font-size:16px";
    document.body.appendChild(el);
    const draw = (text: string, at: number) => {
      el.innerHTML = flavor.draw(text, null);
      el.focus();
      select(el, at);
    };
    const A = "ab:pm-star:cd";
    draw(`${A}\n:pm-heart:`, 0);
    check(`${name} 画成图标`, el.querySelectorAll(".md-ico svg").length, 2);
    check(`${name} 字没变`, serialize(el), `${A}\n:pm-heart:`);
    select(el, 2);
    check(`${name} 光标在图标前`, selectionIn(el), [2, 2]);
    select(el, 11);
    check(`${name} 光标在图标后`, selectionIn(el), [11, 11]);
    document.execCommand("insertText", false, "X");
    check(`${name} 在图标后打字`, serialize(el), `ab:pm-star:Xcd\n:pm-heart:`);
    draw(`${A}\n:pm-heart:`, 11);
    document.execCommand("delete");
    check(`${name} 退格删掉整个图标`, serialize(el), `abcd\n:pm-heart:`);
    draw(`${A}\n:pm-heart:`, 2);
    document.execCommand("insertText", false, "Y");
    check(`${name} 在图标前打字`, serialize(el), `abY:pm-star:cd\n:pm-heart:`);
    const all = `${A}\n:pm-heart:`;
    draw(all, all.length);
    check(`${name} 光标在行尾的图标后`, selectionIn(el), [all.length, all.length]);
    document.execCommand("insertText", false, "Z");
    check(`${name} 在行尾的图标后打字`, serialize(el), `${all}Z`);
    draw(all, A.length + 1);
    check(`${name} 光标在行首的图标前`, selectionIn(el), [A.length + 1, A.length + 1]);
    document.execCommand("insertText", false, "Q");
    check(`${name} 在行首的图标前打字`, serialize(el), `${A}\nQ:pm-heart:`);
    draw(all, 0);
    select(el, 1, 12);
    check(`${name} 选区跨过图标`, selectionIn(el), [1, 12]);
    el.remove();
  }
  await fetch(`http://127.0.0.1:${port}/`, { method: "POST", mode: "no-cors", body: JSON.stringify({ agent: navigator.userAgent, pass: rows.filter((r) => r.ok).length, fail: rows.filter((r) => !r.ok), total: rows.length }) }).catch(() => {});
  document.title = `edtest ${rows.filter((r) => !r.ok).length ? "FAIL" : "ok"}`;
  (window as unknown as { __edtest?: Row[] }).__edtest = rows;
}
