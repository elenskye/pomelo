// Pub2MD 预览: an output shown as Mathpix shows it — mathpix-markdown-it's browser bundle (loaded on first
// use, not in the app's own bundle), its HTML in a sandboxed frame (no scripts: nothing in a document can
// reach the app; raw HTML in the text is escaped too). The bundle's styles are taken once and kept out of
// the app's page. Images come inlined from Rust (data: links).
import { useEffect, useState } from "react";
import bundleUrl from "mathpix-markdown-it/es5/bundle.js?url";

type Bundle = { loadMathJax: (noClickHandler?: boolean) => boolean; markdownToHTML: (text: string, options: object) => string };
type Renderer = { html: (text: string) => string; css: string };

let loading: Promise<Renderer> | null = null;

function renderer(): Promise<Renderer> {
  loading ??= new Promise<Renderer>((resolve, reject) => {
    const script = document.createElement("script");
    script.src = bundleUrl;
    script.onload = () => {
      const w = window as unknown as Bundle;
      w.loadMathJax(true);
      const css = ["MJX-SVG-styles", "SVG-styles", "Mathpix-styles"]
        .map((id) => {
          const el = document.getElementById(id);
          const text = el?.textContent ?? "";
          el?.remove();
          return text;
        })
        .join("\n");
      resolve({ html: (text) => w.markdownToHTML(text, { htmlTags: false, width: 760 }), css });
    };
    script.onerror = () => {
      loading = null;
      reject(new Error("预览的显示库没能加载"));
    };
    document.head.appendChild(script);
  });
  return loading;
}

const PAGE = "body{margin:0;padding:28px 36px;background:#fff;color:#1d1d1f;font:15px/1.75 -apple-system,'PingFang SC',sans-serif}img{max-width:100%}table{border-collapse:collapse}";

/** The text rendered; "" while the bundle loads. */
export function MmdPreview(props: { text: string; title: string }) {
  const [doc, setDoc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setDoc(null);
    renderer().then(
      (r) => live && setDoc(`<!doctype html><html><head><meta charset="utf-8"><style>${r.css}</style><style>${PAGE}</style></head><body><div id="preview" class="preview"><div id="preview-content">${r.html(props.text)}</div></div></body></html>`),
      (e: Error) => live && setError(e.message),
    );
    return () => {
      live = false;
    };
  }, [props.text]);
  if (error) return <p className="p-5 text-sm text-destructive">{error}</p>;
  if (doc == null) return <p className="p-5 text-sm text-muted-foreground">正在准备预览…</p>;
  return <iframe title={props.title} sandbox="" srcDoc={doc} className="h-full w-full rounded-b-card bg-white" />;
}
